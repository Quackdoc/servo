/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::borrow::ToOwned;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::thread;

use ipc_channel::ipc::{self, IpcReceiver, IpcSender};
use malloc_size_of::MallocSizeOf;
use net_traits::storage_thread::{StorageThreadMsg, StorageType};
use profile_traits::mem::{
    ProcessReports, ProfilerChan as MemProfilerChan, Report, ReportKind, perform_memory_report,
};
use profile_traits::path;
use servo_url::ServoUrl;

use crate::resource_thread;

const QUOTA_SIZE_LIMIT: usize = 5 * 1024 * 1024;

pub trait StorageThreadFactory {
    fn new(config_dir: Option<PathBuf>, mem_profiler_chan: MemProfilerChan) -> Self;
}

impl StorageThreadFactory for IpcSender<StorageThreadMsg> {
    /// Create a storage thread
    fn new(
        config_dir: Option<PathBuf>,
        mem_profiler_chan: MemProfilerChan,
    ) -> IpcSender<StorageThreadMsg> {
        let (chan, port) = ipc::channel().unwrap();
        let chan2 = chan.clone();
        thread::Builder::new()
            .name("StorageManager".to_owned())
            .spawn(move || {
                mem_profiler_chan.run_with_memory_reporting(
                    || StorageManager::new(port, config_dir).start(),
                    String::from("storage-reporter"),
                    chan2,
                    StorageThreadMsg::CollectMemoryReport,
                );
            })
            .expect("Thread spawning failed");
        chan
    }
}

struct StorageManager {
    port: IpcReceiver<StorageThreadMsg>,
    session_data: HashMap<String, (usize, BTreeMap<String, String>)>,
    local_data: HashMap<String, (usize, BTreeMap<String, String>)>,
    config_dir: Option<PathBuf>,
}

impl StorageManager {
    fn new(port: IpcReceiver<StorageThreadMsg>, config_dir: Option<PathBuf>) -> StorageManager {
        let mut local_data = HashMap::new();
        if let Some(ref config_dir) = config_dir {
            resource_thread::read_json_from_file(&mut local_data, config_dir, "local_data.json");
        }
        StorageManager {
            port,
            session_data: HashMap::new(),
            local_data,
            config_dir,
        }
    }
}

impl StorageManager {
    fn start(&mut self) {
        loop {
            match self.port.recv().unwrap() {
                StorageThreadMsg::Length(sender, url, storage_type) => {
                    self.length(sender, url, storage_type)
                },
                StorageThreadMsg::Key(sender, url, storage_type, index) => {
                    self.key(sender, url, storage_type, index)
                },
                StorageThreadMsg::Keys(sender, url, storage_type) => {
                    self.keys(sender, url, storage_type)
                },
                StorageThreadMsg::SetItem(sender, url, storage_type, name, value) => {
                    self.set_item(sender, url, storage_type, name, value);
                    self.save_state()
                },
                StorageThreadMsg::GetItem(sender, url, storage_type, name) => {
                    self.request_item(sender, url, storage_type, name)
                },
                StorageThreadMsg::RemoveItem(sender, url, storage_type, name) => {
                    self.remove_item(sender, url, storage_type, name);
                    self.save_state()
                },
                StorageThreadMsg::Clear(sender, url, storage_type) => {
                    self.clear(sender, url, storage_type);
                    self.save_state()
                },
                StorageThreadMsg::CollectMemoryReport(sender) => {
                    let reports = self.collect_memory_reports();
                    sender.send(ProcessReports::new(reports));
                },
                StorageThreadMsg::Exit(sender) => {
                    // Nothing to do since we save localstorage set eagerly.
                    let _ = sender.send(());
                    break;
                },
            }
        }
    }

    fn collect_memory_reports(&self) -> Vec<Report> {
        let mut reports = vec![];
        perform_memory_report(|ops| {
            reports.push(Report {
                path: path!["storage", "local"],
                kind: ReportKind::ExplicitJemallocHeapSize,
                size: self.local_data.size_of(ops),
            });

            reports.push(Report {
                path: path!["storage", "session"],
                kind: ReportKind::ExplicitJemallocHeapSize,
                size: self.session_data.size_of(ops),
            });
        });
        reports
    }

    fn save_state(&self) {
        if let Some(ref config_dir) = self.config_dir {
            resource_thread::write_json_to_file(&self.local_data, config_dir, "local_data.json");
        }
    }

    fn select_data(
        &self,
        storage_type: StorageType,
    ) -> &HashMap<String, (usize, BTreeMap<String, String>)> {
        match storage_type {
            StorageType::Session => &self.session_data,
            StorageType::Local => &self.local_data,
        }
    }

    fn select_data_mut(
        &mut self,
        storage_type: StorageType,
    ) -> &mut HashMap<String, (usize, BTreeMap<String, String>)> {
        match storage_type {
            StorageType::Session => &mut self.session_data,
            StorageType::Local => &mut self.local_data,
        }
    }

    fn length(&self, sender: IpcSender<usize>, url: ServoUrl, storage_type: StorageType) {
        let origin = self.origin_as_string(url);
        let data = self.select_data(storage_type);
        sender
            .send(data.get(&origin).map_or(0, |(_, entry)| entry.len()))
            .unwrap();
    }

    fn key(
        &self,
        sender: IpcSender<Option<String>>,
        url: ServoUrl,
        storage_type: StorageType,
        index: u32,
    ) {
        let origin = self.origin_as_string(url);
        let data = self.select_data(storage_type);
        let key = data
            .get(&origin)
            .and_then(|(_, entry)| entry.keys().nth(index as usize))
            .cloned();
        sender.send(key).unwrap();
    }

    fn keys(&self, sender: IpcSender<Vec<String>>, url: ServoUrl, storage_type: StorageType) {
        let origin = self.origin_as_string(url);
        let data = self.select_data(storage_type);
        let keys = data
            .get(&origin)
            .map_or(vec![], |(_, entry)| entry.keys().cloned().collect());

        sender.send(keys).unwrap();
    }

    /// Sends Ok(changed, Some(old_value)) in case there was a previous
    /// value with the same key name but with different value name
    /// otherwise sends Err(()) to indicate that the operation would result in
    /// exceeding the quota limit
    fn set_item(
        &mut self,
        sender: IpcSender<Result<(bool, Option<String>), ()>>,
        url: ServoUrl,
        storage_type: StorageType,
        name: String,
        value: String,
    ) {
        let origin = self.origin_as_string(url);

        let (this_storage_size, other_storage_size) = {
            let local_data = self.select_data(StorageType::Local);
            let session_data = self.select_data(StorageType::Session);
            let local_data_size = local_data.get(&origin).map_or(0, |&(total, _)| total);
            let session_data_size = session_data.get(&origin).map_or(0, |&(total, _)| total);
            match storage_type {
                StorageType::Local => (local_data_size, session_data_size),
                StorageType::Session => (session_data_size, local_data_size),
            }
        };

        let data = self.select_data_mut(storage_type);
        if !data.contains_key(&origin) {
            data.insert(origin.clone(), (0, BTreeMap::new()));
        }

        let message = data
            .get_mut(&origin)
            .map(|&mut (ref mut total, ref mut entry)| {
                let mut new_total_size = this_storage_size + value.len();
                if let Some(old_value) = entry.get(&name) {
                    new_total_size -= old_value.len();
                } else {
                    new_total_size += name.len();
                }

                if (new_total_size + other_storage_size) > QUOTA_SIZE_LIMIT {
                    return Err(());
                }

                let message =
                    entry
                        .insert(name.clone(), value.clone())
                        .map_or(Ok((true, None)), |old| {
                            if old == value {
                                Ok((false, None))
                            } else {
                                Ok((true, Some(old)))
                            }
                        });
                *total = new_total_size;
                message
            })
            .unwrap();
        sender.send(message).unwrap();
    }

    fn request_item(
        &self,
        sender: IpcSender<Option<String>>,
        url: ServoUrl,
        storage_type: StorageType,
        name: String,
    ) {
        let origin = self.origin_as_string(url);
        let data = self.select_data(storage_type);
        sender
            .send(
                data.get(&origin)
                    .and_then(|(_, entry)| entry.get(&name))
                    .cloned(),
            )
            .unwrap();
    }

    /// Sends Some(old_value) in case there was a previous value with the key name, otherwise sends None
    fn remove_item(
        &mut self,
        sender: IpcSender<Option<String>>,
        url: ServoUrl,
        storage_type: StorageType,
        name: String,
    ) {
        let origin = self.origin_as_string(url);
        let data = self.select_data_mut(storage_type);
        let old_value = data
            .get_mut(&origin)
            .and_then(|&mut (ref mut total, ref mut entry)| {
                entry.remove(&name).inspect(|old| {
                    *total -= name.len() + old.len();
                })
            });
        sender.send(old_value).unwrap();
    }

    fn clear(&mut self, sender: IpcSender<bool>, url: ServoUrl, storage_type: StorageType) {
        let origin = self.origin_as_string(url);
        let data = self.select_data_mut(storage_type);
        sender
            .send(
                data.get_mut(&origin)
                    .is_some_and(|&mut (ref mut total, ref mut entry)| {
                        if !entry.is_empty() {
                            entry.clear();
                            *total = 0;
                            true
                        } else {
                            false
                        }
                    }),
            )
            .unwrap();
    }

    fn origin_as_string(&self, url: ServoUrl) -> String {
        url.origin().ascii_serialization()
    }
}
