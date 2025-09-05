use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::{thread};
use magnus::{embed, Ruby, Error as MagnusError, define_global_function, function, RHash, RString, Value, RArray};
use serde::{Deserialize, Serialize};
use evdev::EventType;

// Commands sent to the Ruby thread
#[derive(Debug)]
enum RubyCommand {
  LoadScript { name: String, path: String },
  StartEventLoop,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicalEvent {
  pub script: String,
  pub event_type: u16,
  pub code: u16,
  pub value: i32,
  pub timestamp_sec: u64,
  pub timestamp_nsec: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyntheticEvent {
  pub event_type: u16,
  pub code: u16,
  pub value: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StateQuery {
  KeyState(u16),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StateResponse {
  KeyState(bool),
}

pub struct RubyService {
  command_sender: Sender<RubyCommand>,
  synthetic_receiver: Arc<Mutex<Receiver<SyntheticEvent>>>,
  state_handler: Arc<dyn Fn(StateQuery) -> StateResponse + Send + Sync>,
}

// Global state for Ruby callbacks (needed because Magnus callbacks are global)
static mut SYNTHETIC_SENDER: Option<Arc<Mutex<Sender<SyntheticEvent>>>> = None;
static mut STATE_HANDLER: Option<Arc<dyn Fn(StateQuery) -> StateResponse + Send + Sync>> = None;
static mut EVENT_QUEUE: Option<Arc<Mutex<Vec<PhysicalEvent>>>> = None;

impl RubyService {
  pub fn new<F>(state_handler: F) -> Result<RubyService, Box<dyn std::error::Error>>
  where
    F: Fn(StateQuery) -> StateResponse + Send + Sync + 'static,
  {
    let (command_sender, command_receiver) = mpsc::channel::<RubyCommand>();
    let (synthetic_sender, synthetic_receiver) = mpsc::channel::<SyntheticEvent>();

    let synthetic_receiver = Arc::new(Mutex::new(synthetic_receiver));
    let synthetic_sender = Arc::new(Mutex::new(synthetic_sender));
    let state_handler = Arc::new(state_handler);

    // Set up global state for Ruby callbacks
    unsafe {
      SYNTHETIC_SENDER = Some(synthetic_sender.clone());
      STATE_HANDLER = Some(state_handler.clone());
      EVENT_QUEUE = Some(Arc::new(Mutex::new(Vec::new())));
    }

    thread::spawn(move || {
      Self::ruby_thread_main(command_receiver);
    });

    Ok(RubyService {
      command_sender,
      synthetic_receiver,
      state_handler,
    })
  }

  fn ruby_thread_main(command_receiver: Receiver<RubyCommand>) {
    let cleanup = unsafe { embed::init() };
    let ruby = &*cleanup;

    if let Err(e) = Self::setup_ruby_environment(ruby) {
      eprintln!("[Ruby runtime] Failed to setup Ruby environment: {}", e);
      std::process::exit(1);
    }

    for command in command_receiver {
      println!("[Ruby runtime] Received command: {:?}", command);
      match command {
        RubyCommand::LoadScript { name, path } => {
          let script = format!("$makita_runtime.load_script('{}', '{}')", name, path);
          if let Err(e) = ruby.eval::<Value>(&script) {
            eprintln!("[Ruby runtime] Failed to load script: {}", e);
          }
        }
        RubyCommand::StartEventLoop => {
          if let Err(e) = ruby.eval::<Value>("$makita_runtime.start_event_loop") {
            eprintln!("[Ruby runtime] Failed to start event loop: {}", e);
          }
        }
      }
    }
  }

  fn setup_ruby_environment(ruby: &Ruby) -> Result<(), MagnusError> {
    define_global_function("makita_send_synthetic_event", function!(ruby_send_synthetic_event, 3));
    define_global_function("makita_query_state", function!(ruby_query_state, 2));
    define_global_function("makita_log", function!(ruby_log_message, 2));
    define_global_function("makita_get_events", function!(ruby_get_events, 0));

    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/compatibility.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/selector.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/timeout.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/timeouts.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/fiber_scheduler.rb"))?;

    let _: Value = ruby.eval(include_str!("../ruby/event_loop.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/event_codes.rb"))?;

    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_KEY, {})", EventType::KEY.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_RELATIVE, {})", EventType::RELATIVE.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_ABSOLUTE, {})", EventType::ABSOLUTE.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_SWITCH, {})", EventType::SWITCH.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_LED, {})", EventType::LED.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_SOUND, {})", EventType::SOUND.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_FORCEFEEDBACKSTATUS, {})", EventType::FORCEFEEDBACKSTATUS.0).as_str())?;

    let _: Value = ruby.eval("$makita_runtime = MagnusRuntime.new")?;

    Ok(())
  }

  pub fn start_event_loop(&self) -> Result<(), Box<dyn std::error::Error>> {
    println!("[Ruby runtime] Starting event loop...");
    self.command_sender.send(RubyCommand::StartEventLoop)?;
    Ok(())
  }

  pub fn load_script(&self, name: String, path: String) -> Result<(), Box<dyn std::error::Error>> {
    println!("[Ruby runtime] Loading script: {} from {}", name, path);
    self.command_sender.send(RubyCommand::LoadScript { name, path })?;
    Ok(())
  }

  pub fn send_event(&self, event: PhysicalEvent) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
      if let Some(queue) = &EVENT_QUEUE {
        if let Ok(mut queue) = queue.lock() {
          queue.push(event);
        }
      }
    }
    Ok(())
  }

  pub fn receive_synthetic_events(&self) -> Vec<SyntheticEvent> {
    let mut events = Vec::new();
    if let Ok(receiver) = self.synthetic_receiver.lock() {
      while let Ok(event) = receiver.try_recv() {
        events.push(event);
      }
    }
    events
  }
}

// Ruby callback functions
fn ruby_send_synthetic_event(event_type: u16, code: u16, value: i32) -> Result<(), MagnusError> {
  unsafe {
    if let Some(sender) = &SYNTHETIC_SENDER {
      if let Ok(sender) = sender.lock() {
        let event = SyntheticEvent {
          event_type,
          code,
          value,
        };
        let _ = sender.send(event);
      }
    }
  }
  Ok(())
}

fn ruby_query_state(query_type: RString, key_code: Option<u16>) -> Result<RString, MagnusError> {
  unsafe {
    if let Some(handler) = &STATE_HANDLER {
      let query_str = query_type.to_string()?;
      let query = match query_str.as_str() {
        "KeyState" => {
          if let Some(code) = key_code {
            StateQuery::KeyState(code)
          } else {
            return Ok(RString::new("false"));
          }
        },
        _ => return Ok(RString::new("false")),
      };

      let response = handler(query);
      let result = match response {
        StateResponse::KeyState(pressed) => pressed.to_string(),
      };
      return Ok(RString::new(&result));
    }
  }
  Ok(RString::new("false"))
}

fn ruby_log_message(level: RString, message: RString) -> Result<(), MagnusError> {
  let level_str = level.to_string()?;
  let message_str = message.to_string()?;

  match level_str.as_str() {
    "error" => eprintln!("[Ruby:error] {}", message_str),
    "warn" => eprintln!("[Ruby:warn] {}", message_str),
    "info" => println!("[Ruby:info] {}", message_str),
    "debug" => println!("[Ruby:debug] {}", message_str),
    _ => println!("[Ruby:{}] {}", level_str, message_str),
  }

  Ok(())
}

fn ruby_get_events() -> Result<RArray, MagnusError> {
  unsafe {
    if let Some(queue) = &EVENT_QUEUE {
      if let Ok(mut queue) = queue.lock() {
        let events: Vec<PhysicalEvent> = queue.drain(..).collect();

        let ruby_array = RArray::new();
        for event in events {
          let hash = RHash::new();
          hash.aset("script", event.script)?;
          hash.aset("event_type", event.event_type)?;
          hash.aset("code", event.code)?;
          hash.aset("value", event.value)?;
          hash.aset("timestamp_sec", event.timestamp_sec)?;
          hash.aset("timestamp_nsec", event.timestamp_nsec)?;
          ruby_array.push(hash)?;
        }

        return Ok(ruby_array);
      }
    }
  }

  // Return empty array if queue is not available or locked
  Ok(RArray::new())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  #[test]
  fn test_magnus_ruby_service_creation() {
    let service = RubyService::new(|query| match query {
      StateQuery::KeyState(_) => StateResponse::KeyState(false),
    });

    assert!(service.is_ok());
  }

  #[test]
  fn test_command_sending() {
    let service = RubyService::new(|query| match query {
      StateQuery::KeyState(_) => StateResponse::KeyState(false),
    }).expect("Failed to create service");

    // Test script loading
    let result = service.load_script("test".to_string(), "/tmp/test.rb".to_string());
    assert!(result.is_ok());

    // Test event sending
    let event = PhysicalEvent {
      script: "test".to_string(),
      event_type: 1,
      code: 30,
      value: 1,
      timestamp_sec: 0,
      timestamp_nsec: 0,
    };
    let result = service.send_event(event);
    assert!(result.is_ok());

    // Test event loop start
    let result = service.start_event_loop();
    assert!(result.is_ok());
  }

  #[test]
  fn test_synthetic_event_reception() {
    let service = RubyService::new(|query| match query {
      StateQuery::KeyState(_) => StateResponse::KeyState(false),
    }).expect("Failed to create service");

    // Initially should have no events
    let events = service.receive_synthetic_events();
    assert!(events.is_empty());
  }
}
