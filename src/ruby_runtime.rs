use magnus::{embed, function, Value, Error, prelude::*};
use std::sync::mpsc::{self, Sender, Receiver};
use std::thread;
use std::cell::RefCell;
use evdev::Key;

/// Actions that Ruby can request
#[derive(Clone, Debug)]
pub enum Action {
    Press(u16),
    PressDown(Vec<u16>),
    Release(u16),
}

/// Thread-local queue for Ruby callbacks
thread_local! {
    static ACTIONS: RefCell<Vec<Action>> = RefCell::new(Vec::new());
}

/// Ruby-exposed functions
fn rb_press(key: u16) -> Result<(), Error> {
    ACTIONS.with(|a| a.borrow_mut().push(Action::Press(key)));
    Ok(())
}

fn rb_press_down(k1: u16, k2: u16) -> Result<(), Error> {
    ACTIONS.with(|a| a.borrow_mut().push(Action::PressDown(vec![k1, k2])));
    Ok(())
}

fn rb_release(key: u16) -> Result<(), Error> {
    ACTIONS.with(|a| a.borrow_mut().push(Action::Release(key)));
    Ok(())
}

/// Ruby event wrapper for the handle() function
#[derive(Clone, Debug)]
pub struct RubyEvent {
    pub key_code: Option<u16>,
    pub value: i32,
}

/// Response from Ruby script execution
#[derive(Debug)]
pub struct Response {
    pub consume: bool,
    pub actions: Vec<Action>,
}

/// Ruby service communication
pub enum Request {
    LoadScript { name: String, path: String },
    CallScript { script_name: String, ev: RubyEvent, reply: Sender<Response> },
}

/// Ruby service with dedicated thread
pub struct RubyService {
    tx: Sender<Request>,
}

impl RubyService {
    pub fn new() -> RubyService {
        let (tx, rx): (Sender<Request>, Receiver<Request>) = mpsc::channel();

        thread::spawn(move || {
            // Ruby VM on this thread
            let ruby = unsafe { embed::init() };

            // Define Makita module and methods
            let makita = ruby.define_module("Makita").unwrap();
            makita.define_singleton_method("press", function!(rb_press, 1)).unwrap();
            makita.define_singleton_method("press_down", function!(rb_press_down, 2)).unwrap();
            makita.define_singleton_method("release", function!(rb_release, 1)).unwrap();

            // Key constants
            makita.const_set("KB_LALT", Key::KEY_LEFTALT.code()).unwrap();
            makita.const_set("KB_LTAB", Key::KEY_TAB.code()).unwrap();
            makita.const_set("KB_ESC", Key::KEY_ESC.code()).unwrap();
            makita.const_set("KB_ENTER", Key::KEY_ENTER.code()).unwrap();

            // Predefine Event class once
            ruby.eval::<Value>(r#"
                class Event
                  def initialize(key, value)
                    @key = key
                    @value = value
                  end
                  def key; @key == 0 ? nil : @key; end
                  def key_down?; @value == 1; end
                  def key_up?; @value == 0; end
                end
            "#).unwrap();

            let mut scripts: std::collections::HashMap<String, String> = std::collections::HashMap::new();

            while let Ok(req) = rx.recv() {
                match req {
                    Request::LoadScript { name, path } => {
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                scripts.insert(name.clone(), content);
                                println!("Loaded Ruby script: {} from {}", name, path);
                            }
                            Err(e) => {
                                eprintln!("Failed to load Ruby script {} from {}: {}", name, path, e);
                            }
                        }
                    }
                    Request::CallScript { script_name, ev, reply } => {
                        let mut actions = Vec::new();
                        let consume = if let Some(script_content) = scripts.get(&script_name) {
                            // Reset actions for this call
                            ACTIONS.with(|a| a.borrow_mut().clear());

                            // Instantiate event and call handle
                            let key = ev.key_code.unwrap_or(0);
                            let value = ev.value;
                            let code = format!(r#"
                                event = Event.new({}, {})
                                {}
                                handle(event)
                            "#, key, value, script_content);

                            match ruby.eval::<Value>(&code) {
                                Ok(val) => {
                                    ACTIONS.with(|a| actions = a.borrow().clone());
                                    val.is_nil()
                                }
                                Err(e) => {
                                    eprintln!("Ruby error in script '{}': {:?}", script_name, e);
                                    false
                                }
                            }
                        } else {
                            eprintln!("Ruby script '{}' not found", script_name);
                            false
                        };
                        let _ = reply.send(Response { consume, actions });
                    }
                }
            }
        });

        RubyService { tx }
    }

    pub fn load_script(&self, name: String, path: String) {
        let _ = self.tx.send(Request::LoadScript { name, path });
    }

    pub fn call_script(&self, script_name: String, ev: RubyEvent) -> Response {
        let (rtx, rrx) = mpsc::channel();
        let _ = self.tx.send(Request::CallScript { script_name, ev, reply: rtx });
        // Blocking wait is fine because Ruby runs in its own thread
        rrx.recv().unwrap_or(Response { consume: false, actions: vec![] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use std::sync::Once;

    static INIT: Once = Once::new();

    fn setup_shared_service() -> &'static RubyService {
        static mut SERVICE: Option<RubyService> = None;
        static INIT_SERVICE: Once = Once::new();

        INIT_SERVICE.call_once(|| {
            unsafe {
                SERVICE = Some(RubyService::new());
            }
        });

        unsafe { SERVICE.as_ref().unwrap() }
    }

    #[test]
    fn test_ruby_service_eat_input() {
        let service = setup_shared_service();

        // Give the Ruby thread time to initialize
        thread::sleep(Duration::from_millis(200));

        // Load the eat_input script
        service.load_script("eat_input".to_string(), "test_script".to_string());

        // Give time for script to be loaded (simulate from string for test)
        thread::sleep(Duration::from_millis(100));

        let event = RubyEvent {
            key_code: Some(Key::KEY_ENTER.code()),
            value: 1,
        };

        let response = service.call_script("eat_input".to_string(), event);

        assert!(response.consume, "eat_input script should consume events");
        assert!(response.actions.is_empty(), "eat_input script should not produce actions");
    }

    #[test]
    fn test_ruby_service_propagate_input() {
        let service = setup_shared_service();

        // Give the Ruby thread time to initialize
        thread::sleep(Duration::from_millis(100));

        // Load the propagate_input script
        service.load_script("propagate_input".to_string(), "examples/ruby_scripts/propagate_input.rb".to_string());

        // Give time for script to be loaded
        thread::sleep(Duration::from_millis(50));

        let event = RubyEvent {
            key_code: Some(Key::KEY_ENTER.code()),
            value: 1,
        };

        let response = service.call_script("propagate_input".to_string(), event);

        assert!(!response.consume, "propagate_input script should not consume events");
        assert_eq!(response.actions.len(), 1, "propagate_input script should produce one action");

        match &response.actions[0] {
            Action::Press(code) => {
                assert_eq!(*code, Key::KEY_ENTER.code(), "Should press the same key");
            }
            _ => panic!("Expected Press action"),
        }
    }

    #[test]
    fn test_ruby_service_alt_tab_combo() {
        let service = setup_shared_service();

        // Give the Ruby thread time to initialize
        thread::sleep(Duration::from_millis(100));

        // Load the alt_tab_combo script
        service.load_script("alt_tab_combo".to_string(), "examples/ruby_scripts/alt_tab_combo.rb".to_string());

        // Give time for script to be loaded
        thread::sleep(Duration::from_millis(50));

        let event = RubyEvent {
            key_code: Some(Key::KEY_ENTER.code()),
            value: 1,
        };

        let response = service.call_script("alt_tab_combo".to_string(), event);

        assert!(!response.consume, "alt_tab_combo script should not consume events");
        assert_eq!(response.actions.len(), 1, "alt_tab_combo script should produce one action");

        match &response.actions[0] {
            Action::PressDown(codes) => {
                assert_eq!(codes.len(), 2, "Should press down two keys");
                assert_eq!(codes[0], Key::KEY_LEFTALT.code(), "First key should be left alt");
                assert_eq!(codes[1], Key::KEY_TAB.code(), "Second key should be tab");
            }
            _ => panic!("Expected PressDown action"),
        }
    }
}
