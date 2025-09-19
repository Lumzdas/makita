use std::sync::{Arc, Mutex};
use std::{thread};
use std::any::Any;
use std::os::fd::{AsRawFd, OwnedFd};
use crossbeam_channel::{unbounded, Sender, Receiver};
use magnus::{embed, Ruby, Error as MagnusError, define_global_function, function, RHash, RString, Value, RArray};
use serde::{Deserialize, Serialize};
use evdev::EventType;
use nix::libc::pathconf;
use nix::unistd;

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

lazy_static::lazy_static! {
  static ref PIPE_FDS: Arc<Mutex<(OwnedFd, OwnedFd)>> = Arc::new(Mutex::new(unistd::pipe().expect("Failed to create pipe")));
}

struct PhysicalEventReceiverInstance { receiver: Mutex<Option<Receiver<PhysicalEvent>>> }
impl PhysicalEventReceiverInstance {
  const fn new() -> Self { PhysicalEventReceiverInstance { receiver: Mutex::new(None) } }
  fn set(&self, r: Receiver<PhysicalEvent>) { *self.receiver.lock().unwrap() = Some(r); }
  fn get(&self) -> Receiver<PhysicalEvent> {
    let locked = self.receiver.lock();
    match locked {
      Ok(x) => {
        match x.clone() {
          Some(r) => r,
          None => panic!("PhysicalEvent Receiver not set"),
        }
      },
      Err(error) => panic!("Failed to lock PhysicalEventReceiverInstance: {}", error.to_string())
    }
  }
}
lazy_static::lazy_static! {
  static ref PHYSICAL_EVENT_RECEIVER: PhysicalEventReceiverInstance = PhysicalEventReceiverInstance::new();
}
lazy_static::lazy_static! {
  static ref PHYSICAL_EVENT_SENDER: Sender<PhysicalEvent> = {
    let (s, r) = unbounded();
    PHYSICAL_EVENT_RECEIVER.set(r);
    s
  };
}

struct CommandReceiverInstance { receiver: Mutex<Option<Receiver<RubyCommand>>> }
impl CommandReceiverInstance {
  const fn new() -> Self { CommandReceiverInstance { receiver: Mutex::new(None) } }
  fn set(&self, r: Receiver<RubyCommand>) { *self.receiver.lock().unwrap() = Some(r); }
  fn get(&self) -> Receiver<RubyCommand> { self.receiver.lock().unwrap().clone().expect("Command Receiver not set") }
}
lazy_static::lazy_static! {
  static ref COMMAND_RECEIVER: CommandReceiverInstance = CommandReceiverInstance::new();
}
lazy_static::lazy_static! {
  static ref COMMAND_SENDER: Sender<RubyCommand> = {
    let (s, r) = unbounded();
    COMMAND_RECEIVER.set(r);
    s
  };
}

struct SyntheticEventReceiverInstance { receiver: Mutex<Option<Receiver<SyntheticEvent>>> }
impl SyntheticEventReceiverInstance {
  const fn new() -> Self { SyntheticEventReceiverInstance { receiver: Mutex::new(None) } }
  fn set(&self, r: Receiver<SyntheticEvent>) { println!("SETTING SYNTHETIC EVENT RECEIVER!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");*self.receiver.lock().unwrap() = Some(r); }
  fn get(&self) -> Receiver<SyntheticEvent> { self.receiver.lock().unwrap().clone().expect("SyntheticEvent Receiver not set") }
}
lazy_static::lazy_static! {
  static ref SYNTHETIC_EVENT_RECEIVER: SyntheticEventReceiverInstance = SyntheticEventReceiverInstance::new();
}
lazy_static::lazy_static! {
  static ref SYNTHETIC_EVENT_SENDER: Sender<SyntheticEvent> = {
    let (s, r) = unbounded();
    SYNTHETIC_EVENT_RECEIVER.set(r);
    s
  };
}

pub struct RubyService {}
impl RubyService {
  pub fn new() -> Result<RubyService, Box<dyn std::error::Error>> {
    println!("Initializing lazy_static channels and starting Ruby thread...");
    println!("Setting up {}", SYNTHETIC_EVENT_SENDER.len());
    println!("Setting up {}", PHYSICAL_EVENT_SENDER.len());
    println!("Setting up {}", COMMAND_SENDER.len());

    thread::spawn(move || { Self::ruby_thread_main(COMMAND_RECEIVER.get()); });
    Ok(RubyService {})
  }

  fn ruby_thread_main(command_receiver: Receiver<RubyCommand>) {
    let cleanup = unsafe { embed::init() };
    let ruby = &*cleanup;

    if let Err(e) = Self::setup_ruby_environment(ruby) {
      eprintln!("[RubyRuntime] Failed to setup Ruby environment: {}", e);
      std::process::exit(1);
    }

    for command in command_receiver {
      println!("[RubyRuntime] Received command: {:?}", command);
      match command {
        RubyCommand::LoadScript { name, path } => {
          let script = format!("$makita_runtime.load_script('{}', '{}')", name, path);
          if let Err(e) = ruby.eval::<Value>(&script) {
            eprintln!("[RubyRuntime] Failed to load script: {}", e);
            std::process::exit(1);
          }
        }
        RubyCommand::StartEventLoop => {
          let _ = ruby.eval::<Value>("$makita_runtime.start_event_loop");
        }
      }
    }
  }

  fn setup_ruby_environment(ruby: &Ruby) -> Result<(), MagnusError> {
    define_global_function("makita_get_signal_pipe_read_fd", function!(ruby_get_signal_pipe_read_fd, 0));
    define_global_function("makita_log", function!(ruby_log_message, 2));
    define_global_function("makita_send_synthetic_event", function!(ruby_send_synthetic_event, 3));
    define_global_function("makita_get_events", function!(ruby_get_events, 0));

    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/compatibility.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/selector.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/timeout.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/timeouts.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/fiber_scheduler/fiber_scheduler.rb"))?;

    let _: Value = ruby.eval(include_str!("../ruby/event.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/makita.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/event_loop.rb"))?;
    let _: Value = ruby.eval(include_str!("../ruby/event_codes.rb"))?;

    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_KEY, {})", EventType::KEY.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_RELATIVE, {})", EventType::RELATIVE.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_ABSOLUTE, {})", EventType::ABSOLUTE.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_SWITCH, {})", EventType::SWITCH.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_LED, {})", EventType::LED.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_SOUND, {})", EventType::SOUND.0).as_str())?;
    let _: Value = ruby.eval(format!("Makita.const_set(:EVENT_TYPE_FORCEFEEDBACKSTATUS, {})", EventType::FORCEFEEDBACKSTATUS.0).as_str())?;

    let _: Value = ruby.eval("$makita_runtime = Runtime.new")?;

    Ok(())
  }

  pub fn start_event_loop(&self) {
    println!("[RubyRuntime] Starting event loop...");
    COMMAND_SENDER.send(RubyCommand::StartEventLoop).expect("failed to start event loop");
  }

  pub fn load_script(&self, name: String, path: String) {
    println!("[RubyRuntime] Loading script: {} from {}", name, path);
    COMMAND_SENDER.send(RubyCommand::LoadScript { name, path }).expect("failed to load script");
  }

  pub fn send_event(&self, event: PhysicalEvent) {
    PHYSICAL_EVENT_SENDER.send(event).unwrap();
    self.signal_that_events_are_available();
  }

  pub fn get_synthetic_event_receiver(&self) -> Receiver<SyntheticEvent> {
    SYNTHETIC_EVENT_RECEIVER.get()
  }

  fn signal_that_events_are_available(&self) {
    let producer_pipe_write_fd = PIPE_FDS.lock().unwrap().1.try_clone().expect("Failed to clone PIPE_FDS");
    unistd::write(producer_pipe_write_fd, &[1u8]).expect("Failed to write to producer pipe");
  }
}

fn ruby_get_signal_pipe_read_fd() -> Result<i32, MagnusError> {
  Ok(PIPE_FDS.lock().unwrap().0.as_raw_fd())
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

fn ruby_send_synthetic_event(event_type: u16, code: u16, value: i32) {
  println!("[Ruby] Sending synthetic event: type={}, code={}, value={}", event_type, code, value);
  SYNTHETIC_EVENT_SENDER.send(SyntheticEvent { event_type, code, value }).unwrap();
}

fn ruby_get_events() -> Result<RArray, MagnusError> {
  let ruby_array = RArray::new();
  for event in PHYSICAL_EVENT_RECEIVER.get().try_iter() {
    let hash = RHash::new();
    hash.aset("script", event.script)?;
    hash.aset("event_type", event.event_type)?;
    hash.aset("code", event.code)?;
    hash.aset("value", event.value)?;
    hash.aset("timestamp_sec", event.timestamp_sec)?;
    hash.aset("timestamp_nsec", event.timestamp_nsec)?;
    ruby_array.push(hash)?;
  }
  Ok(ruby_array)
}
