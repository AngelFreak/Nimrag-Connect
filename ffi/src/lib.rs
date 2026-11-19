//! UniFFI surface over `nimrag_core::client` for the Android app.
//!
//! The whole API is: build a `NimragClient` per GATT connection, feed it
//! `Input`s with `handle(now, input)`, and perform the returned `Step`
//! (writes, events, wake-up deadline). No FIT parsing is exposed: the Android
//! build never contains the locally generated FIT profile (Garmin data).

#[cfg(all(feature = "test-fixtures", target_os = "android"))]
compile_error!("the `test-fixtures` feature (fake watch) must never be built for Android");

mod convert;
#[cfg(feature = "test-fixtures")]
pub mod fixtures;
mod types;

use std::sync::{Arc, Mutex};

use nimrag_core::client as core;

pub use types::*;

uniffi::setup_scaffolding!();

/// Version of this library (the workspace crate version).
#[uniffi::export]
pub fn ffi_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// True if a generated FIT profile was compiled in. Must be false in every
/// Android build; exported so a test can prove it on the real library.
#[uniffi::export]
pub fn fit_profile_compiled_in() -> bool {
    nimrag_core::fit::profile::available()
}

/// Recognise a Garmin device from its advertisement (device catalogue,
/// `nimrag_core::devices::identify`). `manufacturer_data` is the Garmin
/// (company 0x0087) payload WITHOUT the company id. Never fails: unknown
/// devices come back as the generic fallback.
#[uniffi::export]
pub fn identify_device(
    name: Option<String>,
    manufacturer_data: Option<Vec<u8>>,
) -> DeviceModelInfo {
    let model = nimrag_core::devices::identify(name.as_deref(), manufacturer_data.as_deref());
    let (can_sync, unsupported_reason) = match model.sync {
        nimrag_core::devices::SyncSupport::Supported => (true, None),
        nimrag_core::devices::SyncSupport::CannotSyncYet(reason) => {
            (false, Some(reason.to_string()))
        }
    };
    DeviceModelInfo {
        id: model.id.to_string(),
        name: model.name.to_string(),
        generic: std::ptr::eq(model, &nimrag_core::devices::GENERIC),
        can_sync,
        unsupported_reason,
        verified: model.verified,
    }
}

/// One watch link. Create it when the GATT link is up, drop it on disconnect.
///
/// Threading: the object is safe to share, but the protocol is not. The caller
/// must serialise `handle` calls together with performing the returned writes
/// (one at a time, in order); two threads interleaving "handle, then write"
/// can reorder writes on the wire.
#[derive(uniffi::Object)]
pub struct NimragClient {
    inner: Mutex<core::Client>,
}

#[uniffi::export]
impl NimragClient {
    #[uniffi::constructor]
    pub fn new(config: ClientConfig) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(core::Client::new(config.into())),
        })
    }

    /// Feed one input; returns what to write, what happened and when to call
    /// again with `Input::Tick`.
    ///
    /// If an earlier call panicked (poisoned lock) the client fails closed:
    /// this returns an empty `Step` and `phase()` reports `Closed`. The caller
    /// should drop the link and create a new client.
    pub fn handle(&self, now: Now, input: Input) -> Step {
        match self.inner.lock() {
            Ok(mut client) => client.handle(now.into(), input.into()).into(),
            Err(_) => Step {
                writes: Vec::new(),
                events: Vec::new(),
                wake_at_mono_ms: None,
            },
        }
    }

    pub fn phase(&self) -> Phase {
        match self.inner.lock() {
            Ok(client) => client.phase().into(),
            Err(_) => Phase::Closed,
        }
    }
}

#[cfg(test)]
impl NimragClient {
    /// Test hook: poison the lock the way a panic inside `handle` would.
    fn poison(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let _ = std::thread::spawn(move || {
            let _guard = this.inner.lock().unwrap();
            panic!("poison the client lock");
        })
        .join();
    }
}

#[cfg(test)]
mod tests;
