//! Synthetic watch for host tests through the real bindings (feature
//! `test-fixtures`; never compiled into the Android .so). Fake ids only, no
//! capture data. It speaks through the same public codecs as the client
//! (`MlrEndpoint`, COBS, GFDI), so a framing mismatch fails a test instead of
//! hiding in two copies of one bug. Handshake only: no file serving.

use std::sync::{Arc, Mutex};

use nimrag_core::client::reliable::MlrEndpoint;
use nimrag_core::cobs::{self, FrameDecoder};
use nimrag_core::gfdi::Message;
use nimrag_core::messages as m;
use nimrag_core::mlr;
use nimrag_core::multilink::{self, Control};

const REG_HANDLE: u8 = 0x2E;
const GFDI_HANDLE_BYTE: u8 = 0x83;
const KA_HANDLE: u8 = 0x30;

struct Watch {
    mtu: u16,
    endpoint: Option<MlrEndpoint>,
    cobs: FrameDecoder,
    phone_ids: Vec<u16>,
}

/// The watch's scripted side of the bring-up and handshake.
#[derive(uniffi::Object)]
pub struct ScriptedWatch {
    inner: Mutex<Watch>,
}

#[uniffi::export]
impl ScriptedWatch {
    #[uniffi::constructor]
    pub fn new(att_mtu: u16) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Watch {
                mtu: att_mtu,
                endpoint: None,
                cobs: FrameDecoder::new(),
                phone_ids: Vec::new(),
            }),
        })
    }

    /// The phone wrote `value`; returns the notifications to deliver, in order.
    pub fn on_write(&self, now_ms: u64, value: Vec<u8>) -> Vec<Vec<u8>> {
        self.inner.lock().unwrap().on_write(now_ms, &value)
    }

    /// The watch's DeviceInformation (5024), which starts the handshake. Empty
    /// until GFDI is registered.
    pub fn device_information(&self, now_ms: u64) -> Vec<Vec<u8>> {
        let msg = Message::new(5024, m::build_device_information(&fake_device()));
        self.inner.lock().unwrap().send(now_ms, &msg)
    }

    /// GFDI message ids the phone has sent, in order.
    pub fn phone_message_ids(&self) -> Vec<u16> {
        self.inner.lock().unwrap().phone_ids.clone()
    }
}

fn fake_device() -> m::DeviceInformation {
    m::DeviceInformation {
        protocol: 150,
        product: 4242,
        unit_id: 0xC0FF_EE01,
        software: 1234,
        max_packet: 400,
        bluetooth_name: "FakeWatch BT".into(),
        device_name: "Fake".into(),
        model_name: "Watch One".into(),
        tail: vec![0, 0],
    }
}

fn register_response(client: u64, service: u16, handle: u8, flags: u8) -> Vec<u8> {
    let mut v = vec![0, 1];
    v.extend_from_slice(&client.to_le_bytes());
    v.extend_from_slice(&service.to_le_bytes());
    v.extend_from_slice(&[0, handle, flags, 1]);
    v
}

fn close_all_response(client: u64) -> Vec<u8> {
    let mut v = vec![0, 6];
    v.extend_from_slice(&client.to_le_bytes());
    v.extend_from_slice(&[0, 0, 1]);
    v
}

fn response(to: &Message, body: &[u8]) -> Message {
    let mut payload = to.id.to_le_bytes().to_vec();
    payload.push(0);
    payload.extend_from_slice(body);
    Message {
        id: 5000,
        txn: to.txn,
        payload,
    }
}

impl Watch {
    fn send(&mut self, now_ms: u64, msg: &Message) -> Vec<Vec<u8>> {
        let framed = cobs::encode_frame(&msg.encode());
        match self.endpoint.as_mut() {
            Some(e) => e.send(now_ms, &framed),
            None => Vec::new(),
        }
    }

    fn on_write(&mut self, now_ms: u64, value: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        match value.first() {
            None => {}
            Some(0) => self.on_control(value, &mut out),
            Some(b) if b & 0x80 != 0 => self.on_mlr(now_ms, value, &mut out),
            Some(&REG_HANDLE) => {
                let page = value.get(1).copied().unwrap_or(0);
                if page < 5 {
                    out.push(vec![REG_HANDLE, page, 1, 2, 3]);
                } else {
                    out.push(vec![REG_HANDLE, 0xFF, page]);
                }
            }
            Some(&KA_HANDLE) => out.push(vec![KA_HANDLE, 0x02, 0x00]),
            Some(_) => {}
        }
        out
    }

    fn on_control(&mut self, value: &[u8], out: &mut Vec<Vec<u8>>) {
        match multilink::parse_control(value) {
            Some(Control::CloseAllRequest { client_id }) => {
                out.push(close_all_response(client_id));
            }
            Some(Control::RegisterRequest {
                client_id,
                service,
                mode,
            }) => {
                let (handle, flags) = match service {
                    4 => (REG_HANDLE, 0),
                    1 => (GFDI_HANDLE_BYTE, u8::from(mode == 2)),
                    _ => (KA_HANDLE, 0),
                };
                out.push(register_response(client_id, service, handle, flags));
                if service == 1 {
                    self.endpoint = Some(MlrEndpoint::new(GFDI_HANDLE_BYTE & 7, self.mtu));
                }
            }
            _ => {}
        }
    }

    fn on_mlr(&mut self, now_ms: u64, value: &[u8], out: &mut Vec<Vec<u8>>) {
        let Some((header, data)) = mlr::parse(value) else {
            return;
        };
        let Some(ep) = self.endpoint.as_mut() else {
            return;
        };
        let received = ep.receive(now_ms, header, data);
        out.extend(received.writes);
        if received.data.is_empty() {
            return;
        }
        for frame in self.cobs.push(&received.data) {
            let frame = frame.expect("phone sent a valid COBS frame");
            let msg = Message::parse(&frame).expect("phone sent a valid GFDI message");
            self.phone_ids.push(msg.id);
            for reply in self.reply_to(&msg) {
                out.extend(self.send(now_ms, &reply));
            }
        }
    }

    fn reply_to(&self, msg: &Message) -> Vec<Message> {
        match msg.id {
            // Phone's ACK of DeviceInformation: the watch sends its Configuration.
            5000 => match msg.as_response() {
                Some(r) if r.request_id == 5024 => {
                    vec![Message::new(5050, m::build_configuration(&[0xAA, 0xBB]))]
                }
                _ => Vec::new(),
            },
            5031 => vec![response(
                msg,
                &m::build_supported_file_types(&[m::FileType {
                    data_type: 128,
                    sub_type: 4,
                    name: "FIT_TYPE_4".into(),
                }]),
            )],
            5050 | 5030 | 5007 | 5008 | 5043 => vec![response(msg, &[])],
            _ => Vec::new(),
        }
    }
}
