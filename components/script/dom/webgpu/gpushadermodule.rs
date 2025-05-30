/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::rc::Rc;

use dom_struct::dom_struct;
use webgpu_traits::{ShaderCompilationInfo, WebGPU, WebGPURequest, WebGPUShaderModule};

use super::gpucompilationinfo::GPUCompilationInfo;
use crate::dom::bindings::cell::DomRefCell;
use crate::dom::bindings::codegen::Bindings::WebGPUBinding::{
    GPUShaderModuleDescriptor, GPUShaderModuleMethods,
};
use crate::dom::bindings::reflector::{DomGlobal, Reflector, reflect_dom_object};
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::str::USVString;
use crate::dom::bindings::trace::RootedTraceableBox;
use crate::dom::globalscope::GlobalScope;
use crate::dom::promise::Promise;
use crate::dom::types::GPUDevice;
use crate::realms::InRealm;
use crate::routed_promise::{RoutedPromiseListener, route_promise};
use crate::script_runtime::CanGc;

#[dom_struct]
pub(crate) struct GPUShaderModule {
    reflector_: Reflector,
    #[ignore_malloc_size_of = "defined in webgpu"]
    #[no_trace]
    channel: WebGPU,
    label: DomRefCell<USVString>,
    #[no_trace]
    shader_module: WebGPUShaderModule,
    #[ignore_malloc_size_of = "promise"]
    compilation_info_promise: Rc<Promise>,
}

impl GPUShaderModule {
    fn new_inherited(
        channel: WebGPU,
        shader_module: WebGPUShaderModule,
        label: USVString,
        promise: Rc<Promise>,
    ) -> Self {
        Self {
            reflector_: Reflector::new(),
            channel,
            label: DomRefCell::new(label),
            shader_module,
            compilation_info_promise: promise,
        }
    }

    pub(crate) fn new(
        global: &GlobalScope,
        channel: WebGPU,
        shader_module: WebGPUShaderModule,
        label: USVString,
        promise: Rc<Promise>,
        can_gc: CanGc,
    ) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(GPUShaderModule::new_inherited(
                channel,
                shader_module,
                label,
                promise,
            )),
            global,
            can_gc,
        )
    }
}

impl GPUShaderModule {
    pub(crate) fn id(&self) -> WebGPUShaderModule {
        self.shader_module
    }

    /// <https://gpuweb.github.io/gpuweb/#dom-gpudevice-createshadermodule>
    pub(crate) fn create(
        device: &GPUDevice,
        descriptor: RootedTraceableBox<GPUShaderModuleDescriptor>,
        comp: InRealm,
        can_gc: CanGc,
    ) -> DomRoot<GPUShaderModule> {
        let program_id = device.global().wgpu_id_hub().create_shader_module_id();
        let promise = Promise::new_in_current_realm(comp, can_gc);
        let shader_module = GPUShaderModule::new(
            &device.global(),
            device.channel().clone(),
            WebGPUShaderModule(program_id),
            descriptor.parent.label.clone(),
            promise.clone(),
            can_gc,
        );
        let sender = route_promise(
            &promise,
            &*shader_module,
            device
                .global()
                .task_manager()
                .dom_manipulation_task_source(),
        );
        device
            .channel()
            .0
            .send(WebGPURequest::CreateShaderModule {
                device_id: device.id().0,
                program_id,
                program: descriptor.code.0.clone(),
                label: None,
                sender,
            })
            .expect("Failed to create WebGPU ShaderModule");
        shader_module
    }
}

impl GPUShaderModuleMethods<crate::DomTypeHolder> for GPUShaderModule {
    /// <https://gpuweb.github.io/gpuweb/#dom-gpuobjectbase-label>
    fn Label(&self) -> USVString {
        self.label.borrow().clone()
    }

    /// <https://gpuweb.github.io/gpuweb/#dom-gpuobjectbase-label>
    fn SetLabel(&self, value: USVString) {
        *self.label.borrow_mut() = value;
    }

    /// <https://gpuweb.github.io/gpuweb/#dom-gpushadermodule-getcompilationinfo>
    fn GetCompilationInfo(&self) -> Rc<Promise> {
        self.compilation_info_promise.clone()
    }
}

impl RoutedPromiseListener<Option<ShaderCompilationInfo>> for GPUShaderModule {
    fn handle_response(
        &self,
        response: Option<ShaderCompilationInfo>,
        promise: &Rc<Promise>,
        can_gc: CanGc,
    ) {
        let info = GPUCompilationInfo::from(&self.global(), response, can_gc);
        promise.resolve_native(&info, can_gc);
    }
}

impl Drop for GPUShaderModule {
    fn drop(&mut self) {
        if let Err(e) = self
            .channel
            .0
            .send(WebGPURequest::DropShaderModule(self.shader_module.0))
        {
            warn!(
                "Failed to send DropShaderModule ({:?}) ({})",
                self.shader_module.0, e
            );
        }
    }
}
