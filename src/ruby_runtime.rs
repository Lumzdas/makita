use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::process::{Command, Stdio, Child};
use std::io::{BufRead, BufReader, Write, BufWriter};
use std::sync::{Arc, Mutex};
use evdev::InputEvent;
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};
use nix::unistd::{Uid, Gid, User, setuid, setgid, getuid};
use std::env;
use std::os::unix::process::CommandExt;
use nix::libc::exit;

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
  ModifierState,
  DeviceConnected,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StateResponse {
  KeyState(bool),
  ModifierState(Vec<u16>),
  DeviceConnected(bool),
}

pub struct RubyService {
  event_sender: Sender<PhysicalEvent>,
  synthetic_receiver: Arc<Mutex<Receiver<SyntheticEvent>>>,
  script_sender: Sender<ScriptCommand>,
  state_handler: Arc<dyn Fn(StateQuery) -> StateResponse + Send + Sync>,
  ruby_process: Option<Child>,
}

#[derive(Debug)]
pub enum ScriptCommand {
  LoadScript { name: String, path: String },
}

fn get_target_user() -> Result<(Uid, Gid), Box<dyn std::error::Error>> {
  if let Ok(sudo_user) = env::var("SUDO_USER") {
    if let Ok(Some(user)) = User::from_name(&sudo_user) {
      return Ok((user.uid, user.gid));
    }
  }

  Ok((getuid(), nix::unistd::getgid()))
}

impl RubyService {
  pub fn new<F>(state_handler: F) -> Result<RubyService, Box<dyn std::error::Error>>
  where
    F: Fn(StateQuery) -> StateResponse + Send + Sync + 'static,
  {
    let (event_sender, event_receiver) = mpsc::channel::<PhysicalEvent>();
    let (synthetic_sender, synthetic_receiver) = mpsc::channel::<SyntheticEvent>();
    let (script_sender, script_receiver) = mpsc::channel::<ScriptCommand>();

    let synthetic_receiver = Arc::new(Mutex::new(synthetic_receiver));
    let state_handler = Arc::new(state_handler);

    let ruby_process = Self::spawn_ruby_process(
      event_receiver,
      synthetic_sender,
      script_receiver,
      state_handler.clone(),
    )?;

    Ok(RubyService {
      event_sender,
      synthetic_receiver,
      script_sender,
      state_handler,
      ruby_process: Some(ruby_process),
    })
  }

  fn write_ruby_file(suffix: &str, content: &str) {
    let path = &env::temp_dir().join("makita").join(suffix);
    std::fs::write(path, content).expect(&("Failed to write ".to_string() + suffix + path.to_str().unwrap()))
  }

  fn spawn_ruby_process(
    event_receiver: Receiver<PhysicalEvent>,
    synthetic_sender: Sender<SyntheticEvent>,
    script_receiver: Receiver<ScriptCommand>,
    state_handler: Arc<dyn Fn(StateQuery) -> StateResponse + Send + Sync>,
  ) -> Result<Child, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(env::temp_dir().join("makita")).expect("Failed to create /tmp/makita");
    Self::write_ruby_file("makita_event_loop.rb", include_str!("../ruby/event_loop.rb"));

    std::fs::create_dir_all(&env::temp_dir().join("makita/fiber_scheduler")).expect("Failed to create /tmp/makita/fiber_scheduler");
    Self::write_ruby_file("fiber_scheduler/compatibility.rb", include_str!("../ruby/fiber_scheduler/compatibility.rb"));
    Self::write_ruby_file("fiber_scheduler/fiber_scheduler.rb", include_str!("../ruby/fiber_scheduler/fiber_scheduler.rb"));
    Self::write_ruby_file("fiber_scheduler/selector.rb", include_str!("../ruby/fiber_scheduler/selector.rb"));
    Self::write_ruby_file("fiber_scheduler/timeout.rb", include_str!("../ruby/fiber_scheduler/timeout.rb"));
    Self::write_ruby_file("fiber_scheduler/timeouts.rb", include_str!("../ruby/fiber_scheduler/timeouts.rb"));

    let (target_uid, target_gid) = get_target_user()?;

    let mut child = unsafe {
      Command::new("ruby")
        .arg(&env::temp_dir().join("makita/makita_event_loop.rb"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .pre_exec(move || {
          // Drop privileges before executing Ruby
          if getuid().is_root() {
            if let Err(e) = nix::unistd::setgroups(&[target_gid]) {
              eprintln!("Failed to set supplementary groups: {}", e);
            }
            if let Err(e) = setgid(target_gid) {
              eprintln!("Failed to set GID: {}", e);
              return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Failed to drop group privileges"));
            }
            if let Err(e) = setuid(target_uid) {
              eprintln!("Failed to set UID: {}", e);
              return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Failed to drop user privileges"));
            }
            println!("Ruby process will run as UID {} GID {}", target_uid, target_gid);
          }
          Ok(())
        }).spawn()?
    };

    let stdin = child.stdin.take().ok_or("Failed to get Ruby process stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to get Ruby process stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to get Ruby process stderr")?;

    let stdin = Arc::new(Mutex::new(BufWriter::new(stdin)));
    let stdin_clone: Arc<Mutex<BufWriter<std::process::ChildStdin>>> = Arc::clone(&stdin);

    // Thread to send events to Ruby process
    thread::spawn(move || {
      while let Ok(event) = event_receiver.recv() {
        if let Ok(mut writer) = stdin_clone.lock() {
          let json_event = serde_json::to_string(&event).unwrap_or_default();
          let _ = writeln!(writer, "EVENT:{}", json_event);
          let _ = writer.flush();
        }
      }
    });

    // Thread to handle script loading commands
    let stdin_clone2: Arc<Mutex<BufWriter<std::process::ChildStdin>>> = Arc::clone(&stdin);
    thread::spawn(move || {
      while let Ok(command) = script_receiver.recv() {
        if let Ok(mut writer) = stdin_clone2.lock() {
          match command {
            ScriptCommand::LoadScript { name, path } => {
              let _ = writeln!(writer, "LOAD:{}:{}", name, path);
              let _ = writer.flush();
            }
          }
        }
      }
    });

    // Thread to receive synthetic events and state queries from Ruby process
    let stdout_reader = BufReader::new(stdout);
    thread::spawn(move || {
      for line in stdout_reader.lines() {
        if let Ok(line) = line {
          if line.starts_with("SYNTHETIC:") {
            let json = &line[10..];
            if let Ok(event) = serde_json::from_str::<SyntheticEvent>(json) {
              let _ = synthetic_sender.send(event);
            }
          } else if line.starts_with("STATE:") {
            let json = &line[6..];
            if let Ok(query) = serde_json::from_str::<StateQuery>(json) {
              let _response = state_handler(query);
              // TODO: Send response back to Ruby process
            }
          } else if line.starts_with("READY") {
            println!("Ruby event loop is ready");
          } else if line.starts_with("LOADED:") {
            let script_name = &line[7..];
            println!("Ruby script loaded: {}", script_name);
          } else if line.starts_with("ERROR:") {
            eprintln!("Ruby error: {}", &line[6..]);
          } else if line.starts_with("CONSUME:") {
            let script_name = &line[8..];
            println!("Event consumed by script: {}", script_name);
          }
        }
      }
    });

    // Thread to handle stderr
    let stderr_reader = BufReader::new(stderr);
    thread::spawn(move || {
      for line in stderr_reader.lines() {
        if let Ok(line) = line {
          eprintln!("Ruby stderr: {}", line);
        }
      }
    });

    Ok(child)
  }

  pub fn load_script(&self, name: String, path: String) -> Result<(), Box<dyn std::error::Error>> {
    self.script_sender.send(ScriptCommand::LoadScript { name, path })?;
    Ok(())
  }

  pub fn send_event(&self, event: PhysicalEvent) -> Result<(), Box<dyn std::error::Error>> {
    self.event_sender.send(event)?;
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

  pub fn handle_state_query(&self, query: StateQuery) -> StateResponse {
    (self.state_handler)(query)
  }
}

impl Drop for RubyService {
  fn drop(&mut self) {
    if let Some(mut process) = self.ruby_process.take() {
      let _ = process.kill();
      let _ = process.wait();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_physical_event_from_input_event() {
    use evdev::{EventType, InputEvent};
    use std::time::SystemTime;

    let input_event = InputEvent::new(EventType::KEY, 30, 1); // KEY_A press
    let duration = input_event.timestamp().duration_since(UNIX_EPOCH).unwrap_or_default();
    let physical_event = PhysicalEvent {
      script: "test_script".to_string(),
      event_type: input_event.event_type().0,
      code: input_event.code(),
      value: input_event.value(),
      timestamp_sec: duration.as_secs(),
      timestamp_nsec: duration.subsec_nanos(),
    };

    assert_eq!(physical_event.event_type, 1); // EV_KEY
    assert_eq!(physical_event.code, 30); // KEY_A
    assert_eq!(physical_event.value, 1); // Press
  }

  #[test]
  fn test_ruby_service_creation() {
    let service = RubyService::new(|query| match query {
      StateQuery::KeyState(_) => StateResponse::KeyState(false),
      StateQuery::ModifierState => StateResponse::ModifierState(vec![]),
      StateQuery::DeviceConnected => StateResponse::DeviceConnected(true),
    });

    assert!(service.is_ok());
  }
}
