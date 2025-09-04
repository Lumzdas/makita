use crate::magnus_ruby_runtime::{MagnusRubyService, SyntheticEvent};
use crate::virtual_devices::VirtualDevices;
use evdev::{EventType, InputEvent};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

pub struct EventSender {
  ruby_service: Arc<Mutex<MagnusRubyService>>,
  virtual_devices: Arc<Mutex<VirtualDevices>>,
  running: Arc<Mutex<bool>>,
}

impl EventSender {
  pub fn new(ruby_service: Arc<Mutex<MagnusRubyService>>, virtual_devices: Arc<Mutex<VirtualDevices>>) -> Self {
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
      let events = {
        let ruby_service = self.ruby_service.lock().await;
        ruby_service.receive_synthetic_events()
      };

      if !events.is_empty() {
        println!("[EventSender] Received {} synthetic events", events.len());

        for event in events {
          if let Err(e) = self.send_synthetic_event(event).await {
            eprintln!("[EventSender] Error sending synthetic event: {}", e);
          }
        }
      }

      sleep(Duration::from_millis(1)).await;
    }

    println!("[EventSender] Event sender loop stopped");
    Ok(())
  }

  async fn send_synthetic_event(&self, synthetic_event: SyntheticEvent) -> Result<(), Box<dyn std::error::Error>> {
    let mut virtual_devices = self.virtual_devices.lock().await;

    let input_event = InputEvent::new(
      EventType(synthetic_event.event_type),
      synthetic_event.code,
      synthetic_event.value,
    );

    match EventType(synthetic_event.event_type) {
      EventType::KEY => {
        virtual_devices.keys.emit(&[input_event])?;
        println!("[EventSender] Sent KEY event: code={}, value={}", synthetic_event.code, synthetic_event.value);
      }
      EventType::RELATIVE => {
        virtual_devices.axis.emit(&[input_event])?;
        println!("[EventSender] Sent RELATIVE event: code={}, value={}", synthetic_event.code, synthetic_event.value);
      }
      EventType::ABSOLUTE => {
        virtual_devices.abs.emit(&[input_event])?;
        println!("[EventSender] Sent ABSOLUTE event: code={}, value={}", synthetic_event.code, synthetic_event.value);
      }
      EventType::SWITCH => {
        virtual_devices.keys.emit(&[input_event])?;
        println!("[EventSender] Sent SWITCH event: code={}, value={}", synthetic_event.code, synthetic_event.value);
      }
      _ => {
        virtual_devices.keys.emit(&[input_event])?;
        println!("[EventSender] Sent OTHER event (type={}): code={}, value={}", synthetic_event.event_type, synthetic_event.code, synthetic_event.value);
      }
    }

    Ok(())
  }
}
