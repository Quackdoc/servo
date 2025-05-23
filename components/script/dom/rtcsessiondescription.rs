/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use dom_struct::dom_struct;
use js::rust::HandleObject;

use crate::dom::bindings::codegen::Bindings::RTCSessionDescriptionBinding::{
    RTCSdpType, RTCSessionDescriptionInit, RTCSessionDescriptionMethods,
};
use crate::dom::bindings::error::Fallible;
use crate::dom::bindings::reflector::{Reflector, reflect_dom_object_with_proto};
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::str::DOMString;
use crate::dom::window::Window;
use crate::script_runtime::CanGc;

#[dom_struct]
pub(crate) struct RTCSessionDescription {
    reflector: Reflector,
    ty: RTCSdpType,
    sdp: DOMString,
}

impl RTCSessionDescription {
    pub(crate) fn new_inherited(ty: RTCSdpType, sdp: DOMString) -> RTCSessionDescription {
        RTCSessionDescription {
            reflector: Reflector::new(),
            ty,
            sdp,
        }
    }

    fn new(
        window: &Window,
        proto: Option<HandleObject>,
        ty: RTCSdpType,
        sdp: DOMString,
        can_gc: CanGc,
    ) -> DomRoot<RTCSessionDescription> {
        reflect_dom_object_with_proto(
            Box::new(RTCSessionDescription::new_inherited(ty, sdp)),
            window,
            proto,
            can_gc,
        )
    }
}

impl RTCSessionDescriptionMethods<crate::DomTypeHolder> for RTCSessionDescription {
    /// <https://w3c.github.io/webrtc-pc/#dom-sessiondescription>
    fn Constructor(
        window: &Window,
        proto: Option<HandleObject>,
        can_gc: CanGc,
        config: &RTCSessionDescriptionInit,
    ) -> Fallible<DomRoot<RTCSessionDescription>> {
        Ok(RTCSessionDescription::new(
            window,
            proto,
            config.type_,
            config.sdp.clone(),
            can_gc,
        ))
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescription-type>
    fn Type(&self) -> RTCSdpType {
        self.ty
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescription-sdp>
    fn Sdp(&self) -> DOMString {
        self.sdp.clone()
    }
}
