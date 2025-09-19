use crate::ruby_runtime::SyntheticEvent;
use crate::virtual_devices::VirtualDevices;
use evdev::{EventType, InputEvent};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use crossbeam_channel::Receiver;

pub struct EventSender {
  synthetic_event_receiver: Receiver<SyntheticEvent>,
  virtual_devices: Arc<Mutex<VirtualDevices>>,
}

impl EventSender {
  pub fn new(synthetic_event_receiver: Receiver<SyntheticEvent>, virtual_devices: Arc<Mutex<VirtualDevices>>) -> Self {
    Self { synthetic_event_receiver, virtual_devices }
  }

  pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
    loop {
      println!("[EventSender] Waiting for synthetic events");
      let event = self.synthetic_event_receiver.recv().unwrap();
      let input_event = InputEvent::new(EventType(event.event_type), event.code, event.value);

      let mut virtual_devices = self.virtual_devices.lock().unwrap();

      match EventType(event.event_type) {
        EventType::KEY | EventType::SWITCH => virtual_devices.keys.emit(&[input_event]).unwrap(),
        EventType::RELATIVE => virtual_devices.axis.emit(&[input_event]).unwrap(),
        _ => virtual_devices.keys.emit(&[input_event]).unwrap(),
      }

      sleep(Duration::from_micros(10));
    }
  }
}
