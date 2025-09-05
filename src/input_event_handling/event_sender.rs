use crate::ruby_runtime::{RubyService, SyntheticEvent};
use crate::virtual_devices::VirtualDevices;
use evdev::{EventType, InputEvent};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

pub struct EventSender {
  ruby_service: Arc<Mutex<RubyService>>,
  virtual_devices: Arc<Mutex<VirtualDevices>>,
  running: Arc<Mutex<bool>>,
}

impl EventSender {
  pub fn new(ruby_service: Arc<Mutex<RubyService>>, virtual_devices: Arc<Mutex<VirtualDevices>>) -> Self {
    Self {
      ruby_service,
      virtual_devices,
      running: Arc::new(Mutex::new(false)),
    }
  }

  pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
    {
      let mut running = self.running.lock().await;
      *running = true;
    }

    println!("[EventSender] Starting event sender loop");

    while *self.running.lock().await {
      let ruby_events = {
        let ruby_service = self.ruby_service.lock().await;
        ruby_service.receive_synthetic_events()
      };

      if !ruby_events.is_empty() {
        println!("[EventSender] Received {} synthetic events", ruby_events.len());

        let mut virtual_devices = self.virtual_devices.lock().await;
        for event in ruby_events {
          let input_event = InputEvent::new(EventType(event.event_type), event.code, event.value);

          match EventType(event.event_type) {
            EventType::KEY | EventType::SWITCH => virtual_devices.keys.emit(&[input_event])?,
            EventType::RELATIVE => virtual_devices.axis.emit(&[input_event])?,
            _ => virtual_devices.keys.emit(&[input_event])?,
          }

          // Slight delay to prevent missed events.
          // Not sure why this works.
          sleep(Duration::from_nanos(1)).await;
        }
      }

      sleep(Duration::from_millis(1)).await;
    }

    println!("[EventSender] Event sender loop stopped");
    Ok(())
  }
}
