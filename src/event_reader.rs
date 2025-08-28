use crate::active_client::*;
use crate::config::{parse_modifiers, Associations, Axis, Cursor, Event, Relative, Scroll};
use crate::magnus_ruby_runtime::{MagnusRubyService};
use crate::udev_monitor::Environment;
use crate::virtual_devices::VirtualDevices;
use crate::Config;
use evdev::{AbsoluteAxisType, EventStream, EventType, InputEvent, Key, RelativeAxisType};
use fork::{fork, setsid, Fork};
use std::{
  future::Future,
  option::Option,
  pin::Pin,
  process::{Command, Stdio},
  str::FromStr,
  sync::Arc,
  sync::atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

struct Stick {
  function: String,
  sensitivity: u64,
  deadzone: i32,
  activation_modifiers: Vec<Event>,
}

struct Movement {
  speed: i32,
  acceleration: f32,
}

struct Settings {
  lstick: Stick,
  rstick: Stick,
  invert_cursor_axis: bool,
  invert_scroll_axis: bool,
  axis_16_bit: bool,
  stadia: bool,
  cursor: Movement,
  scroll: Movement,
  chain_only: bool,
  layout_switcher: Key,
  notify_layout_switch: bool,
}

pub struct EventReader {
  config: Vec<Config>,
  stream: Arc<Mutex<EventStream>>,
  virt_dev: Arc<Mutex<VirtualDevices>>,
  lstick_position: Arc<Mutex<Vec<i32>>>,
  rstick_position: Arc<Mutex<Vec<i32>>>,
  cursor_movement: Arc<Mutex<(i32, i32)>>,
  scroll_movement: Arc<Mutex<(i32, i32)>>,
  modifiers: Arc<Mutex<Vec<Event>>>,
  modifier_was_activated: Arc<Mutex<bool>>,
  device_is_connected: Arc<Mutex<bool>>,
  active_layout: Arc<Mutex<u16>>,
  current_config: Arc<Mutex<Config>>,
  environment: Environment,
  settings: Settings,
  ruby_service: Option<MagnusRubyService>,
}

impl EventReader {
  pub fn new(
    config: Vec<Config>,
    virt_dev: Arc<Mutex<VirtualDevices>>,
    stream: Arc<Mutex<EventStream>>,
    modifiers: Arc<Mutex<Vec<Event>>>,
    modifier_was_activated: Arc<Mutex<bool>>,
    environment: Environment,
  ) -> Self {
    let mut position_vector: Vec<i32> = Vec::new();
    for i in [0, 0] {
      position_vector.push(i)
    }
    let lstick_position = Arc::new(Mutex::new(position_vector.clone()));
    let rstick_position = Arc::new(Mutex::new(position_vector.clone()));
    let cursor_movement = Arc::new(Mutex::new((0, 0)));
    let scroll_movement = Arc::new(Mutex::new((0, 0)));
    let device_is_connected: Arc<Mutex<bool>> = Arc::new(Mutex::new(true));
    let active_layout: Arc<Mutex<u16>> = Arc::new(Mutex::new(0));

    let current_config: Arc<Mutex<Config>> = Arc::new(Mutex::new(
      config.iter().find(|&x| x.associations == Associations::default()).unwrap().clone()
    ));
    let settings = config.iter().find(|&x| x.associations == Associations::default()).unwrap().settings.clone();

    let lstick_function = settings.get("LSTICK").unwrap_or(&"cursor".to_string()).to_string();
    let lstick_sensitivity: u64 = settings.get("LSTICK_SENSITIVITY").unwrap_or(&"0".to_string()).parse::<u64>().expect("Invalid LSTICK_SENSITIVITY, use integer >= 0");
    let lstick_deadzone: i32 = settings.get("LSTICK_DEADZONE").unwrap_or(&"5".to_string()).parse::<i32>().expect("Invalid LSTICK_DEADZONE, use integer 0 to 128.");
    let lstick_activation_modifiers: Vec<Event> = parse_modifiers(&settings, "LSTICK_ACTIVATION_MODIFIERS");
    let lstick = Stick {
      function: lstick_function,
      sensitivity: lstick_sensitivity,
      deadzone: lstick_deadzone,
      activation_modifiers: lstick_activation_modifiers,
    };

    let rstick_function: String = settings.get("RSTICK").unwrap_or(&"scroll".to_string()).to_string();
    let rstick_sensitivity: u64 = settings.get("RSTICK_SENSITIVITY").unwrap_or(&"0".to_string()).parse::<u64>().expect("Invalid RSTICK_SENSITIVITY, use integer >= 0");
    let rstick_deadzone: i32 = settings.get("RSTICK_DEADZONE").unwrap_or(&"5".to_string()).parse::<i32>().expect("Invalid RSTICK_DEADZONE, use integer 0 to 128.");
    let rstick_activation_modifiers: Vec<Event> = parse_modifiers(&settings, "RSTICK_ACTIVATION_MODIFIERS");
    let rstick = Stick {
      function: rstick_function,
      sensitivity: rstick_sensitivity,
      deadzone: rstick_deadzone,
      activation_modifiers: rstick_activation_modifiers,
    };

    let axis_16_bit: bool = settings.get("16_BIT_AXIS").unwrap_or(&"false".to_string()).parse().expect("Invalid 16_BIT_AXIS use true/false.");
    let stadia: bool = settings.get("STADIA").unwrap_or(&"false".to_string()).parse().expect("Invalid STADIA use true/false.");
    let chain_only: bool = settings.get("CHAIN_ONLY").unwrap_or(&"true".to_string()).parse().expect("Invalid CHAIN_ONLY use true/false.");
    let invert_cursor_axis: bool = settings.get("INVERT_CURSOR_AXIS").unwrap_or(&"false".to_string()).parse().expect("Invalid INVERT_CURSOR_AXIS use true/false.");
    let invert_scroll_axis: bool = settings.get("INVERT_SCROLL_AXIS").unwrap_or(&"false".to_string()).parse().expect("Invalid INVERT_SCROLL_AXIS use true/false.");
    let cursor_speed: i32 = settings.get("CURSOR_SPEED").unwrap_or(&"0".to_string()).parse().expect("Invalid CURSOR_SPEED, use integer.");
    let cursor_acceleration: f32 = settings.get("CURSOR_ACCEL").unwrap_or(&"1".to_string()).parse().expect("Invalid CURSOR_ACCEL, use float 0 to 1.");
    let scroll_speed: i32 = settings.get("SCROLL_SPEED").unwrap_or(&"0".to_string()).parse().expect("Invalid SCROLL_SPEED, use integer.");
    let scroll_acceleration: f32 = settings.get("SCROLL_ACCEL").unwrap_or(&"1".to_string()).parse().expect("Invalid SCROLL_ACCEL, use float 0 to 1.");

    let cursor = Movement {
      speed: cursor_speed,
      acceleration: cursor_acceleration,
    };

    let scroll = Movement {
      speed: scroll_speed,
      acceleration: scroll_acceleration,
    };

    let layout_switcher: Key = Key::from_str(settings.get("LAYOUT_SWITCHER").unwrap_or(&"BTN_0".to_string())).expect("LAYOUT_SWITCHER is not a valid Key.");
    let notify_layout_switch: bool = settings.get("NOTIFY_LAYOUT_SWITCH").unwrap_or(&"false".to_string()).parse().expect("Invalid NOTIFY_LAYOUT_SWITCH use true/false.");

    let settings = Settings {
      lstick,
      rstick,
      invert_cursor_axis,
      invert_scroll_axis,
      axis_16_bit,
      stadia,
      cursor,
      scroll,
      chain_only,
      layout_switcher,
      notify_layout_switch,
    };

    // Initialize Ruby service and load scripts from config
    let ruby_service = {
      // Clone references for the state handler closure
      println!("Initializing Ruby service...");
      let modifiers_ref = Arc::clone(&modifiers);
      println!("Modifiers reference cloned.");
      let device_connected_ref = Arc::clone(&device_is_connected);
      println!("Device connection reference cloned.");

      let service = MagnusRubyService::new(move |query| {
        use crate::magnus_ruby_runtime::{StateQuery, StateResponse};
        match query {
          StateQuery::KeyState(key_code) => {
            // For now, return false - could be enhanced to track actual key states
            StateResponse::KeyState(false)
          }
          StateQuery::ModifierState => {
            // Return current modifier keys
            if let Ok(mods) = modifiers_ref.try_lock() {
              let codes: Vec<u16> = mods.iter().map(|e| match e {
                Event::Key(key) => key.code(),
                _ => 0,
              }).collect();
              StateResponse::ModifierState(codes)
            } else {
              StateResponse::ModifierState(vec![])
            }
          }
          StateQuery::DeviceConnected => {
            if let Ok(connected) = device_connected_ref.try_lock() {
              StateResponse::DeviceConnected(*connected)
            } else {
              StateResponse::DeviceConnected(false)
            }
          }
        }
      }).expect("Failed to create Ruby service");
      let mut has_scripts = false;

      // Load all Ruby scripts from all configs
      for cfg in &config {
        for (_event, modifier_map) in &cfg.bindings.rubies {
          for (_modifiers, script_name) in modifier_map {
            if let Ok(ruby_scripts_path) = std::env::var("MAKITA_RUBY_SCRIPTS") {
              println!("Loading Ruby script: {}", script_name);
              let script_path = format!("{}/{}.rb", ruby_scripts_path, script_name);
              let _ = service.load_script(script_name.clone(), script_path);
              has_scripts = true;
            }
          }
        }
      }

      // Start the Ruby event loop if we have scripts
      if has_scripts {
        println!("Starting Ruby event loop...");
        service.start_event_loop().expect("Failed to start Ruby event loop");
        println!("Ruby service initialized.");
        Some(service)
      } else {
        None
      }
    };

    Self {
      config,
      stream,
      virt_dev,
      lstick_position,
      rstick_position,
      cursor_movement,
      scroll_movement,
      modifiers,
      modifier_was_activated,
      device_is_connected,
      active_layout,
      current_config,
      environment,
      settings,
      ruby_service,
    }
  }

  pub async fn start(&self) {
    println!("{:?} detected, reading events.\n", self.current_config.lock().await.name);

    tokio::join!(
      self.event_loop(),
      self.loop_2d("cursor", self.settings.invert_cursor_axis, 0, 1),
      self.loop_2d("scroll", self.settings.invert_scroll_axis, 12, 11),
      self.key_loop_2d(&self.settings.cursor, &self.cursor_movement, 0, 1),
      self.key_loop_2d(&self.settings.scroll, &self.scroll_movement, 12, 11),
    );
  }

  pub async fn event_loop(&self) {
    let (
      mut dpad_values,
      mut lstick_values,
      mut rstick_values,
      mut triggers_values,
      mut abs_wheel_position,
    ) = ((0, 0), (0, 0), (0, 0), (0, 0), 0);
    let switcher: Key = self.settings.layout_switcher;
    let mut stream = self.stream.lock().await;
    let mut pen_events: Vec<InputEvent> = Vec::new();
    let is_tablet: bool = stream.device().supported_keys().unwrap_or(&evdev::AttributeSet::new()).contains(Key::BTN_TOOL_PEN);
    let mut max_abs_wheel = 0;
    if let Ok(abs_state) = stream.device().get_abs_state() {
      for state in abs_state {
        if state.maximum > max_abs_wheel {
          max_abs_wheel = state.maximum;
        }
      }
    }

    loop {
      let event = match stream.next().await {
        Some(Ok(event)) => event,
        Some(Err(e)) => {
          eprintln!("Error reading event: {}", e);
          continue;
        }
        None => {
          println!("Event stream ended");
          break;
        }
      };
      match (event.event_type(), RelativeAxisType(event.code()), AbsoluteAxisType(event.code()), is_tablet) {
        (EventType::KEY, _, _, _) => match Key(event.code()) {
          Key::BTN_TL2 | Key::BTN_TR2 => {}
          Key::BTN_TOOL_PEN | Key::BTN_TOOL_RUBBER | Key::BTN_TOOL_BRUSH | Key::BTN_TOOL_PENCIL | Key::BTN_TOOL_AIRBRUSH | Key::BTN_TOOL_MOUSE | Key::BTN_TOOL_LENS
          if is_tablet => pen_events.push(event),
          key if key == switcher && event.value() == 1 => self.change_active_layout().await,
          _ => self.convert_event(event, Event::Key(Key(event.code())), event.value(), false).await
        },
        (EventType::RELATIVE, RelativeAxisType::REL_WHEEL | RelativeAxisType::REL_WHEEL_HI_RES, _, _, ) => match event.value() {
          -1 => self.convert_event(event, Event::Axis(Axis::SCROLL_WHEEL_DOWN), 1, true).await,
          1 => self.convert_event(event, Event::Axis(Axis::SCROLL_WHEEL_UP), 1, true).await,
          _ => {}
        },
        (EventType::ABSOLUTE, _, AbsoluteAxisType::ABS_WHEEL, _) => {
          let value = event.value();
          if value != 0 && abs_wheel_position != 0 {
            let gap = value - abs_wheel_position;
            if gap < -max_abs_wheel / 2 {
              self.convert_event(event, Event::Axis(Axis::ABS_WHEEL_CW), 1, true).await;
            } else if gap > max_abs_wheel / 2 {
              self.convert_event(event, Event::Axis(Axis::ABS_WHEEL_CCW), 1, true).await;
            } else if value > abs_wheel_position {
              self.convert_event(event, Event::Axis(Axis::ABS_WHEEL_CW), 1, true).await;
            } else if value < abs_wheel_position {
              self.convert_event(event, Event::Axis(Axis::ABS_WHEEL_CCW), 1, true).await;
            }
          }
          abs_wheel_position = value;
        }
        (EventType::ABSOLUTE, _, AbsoluteAxisType::ABS_MISC, _) => {
          if is_tablet == false && event.value() == 0 {
            abs_wheel_position = 0
          } else {
            self.emit_default_event(event).await;
          }
        }
        (EventType::ABSOLUTE, _, _, true) => pen_events.push(event),
        (_, _, AbsoluteAxisType::ABS_HAT0X, _) => {
          match event.value() {
            -1 => {
              self.convert_event(event, Event::Axis(Axis::BTN_DPAD_LEFT), 1, false).await;
              dpad_values.0 = -1;
            }
            1 => {
              self.convert_event(event, Event::Axis(Axis::BTN_DPAD_RIGHT), 1, false).await;
              dpad_values.0 = 1;
            }
            0 => {
              match dpad_values.0 {
                -1 => self.convert_event(event, Event::Axis(Axis::BTN_DPAD_LEFT), 0, false).await,
                1 => self.convert_event(event, Event::Axis(Axis::BTN_DPAD_RIGHT), 0, false).await,
                _ => {}
              }
              dpad_values.0 = 0;
            }
            _ => {}
          };
        }
        (_, _, AbsoluteAxisType::ABS_HAT0Y, _) => {
          match event.value() {
            -1 => {
              self.convert_event(event, Event::Axis(Axis::BTN_DPAD_UP), 1, false).await;
              dpad_values.1 = -1;
            }
            1 => {
              self.convert_event(event, Event::Axis(Axis::BTN_DPAD_DOWN), 1, false).await;
              dpad_values.1 = 1;
            }
            0 => {
              match dpad_values.1 {
                -1 => self.convert_event(event, Event::Axis(Axis::BTN_DPAD_UP), 0, false).await,
                1 => self.convert_event(event, Event::Axis(Axis::BTN_DPAD_DOWN), 0, false).await,
                _ => {}
              }
              dpad_values.1 = 0;
            }
            _ => {}
          };
        }
        (EventType::ABSOLUTE, _, AbsoluteAxisType::ABS_X | AbsoluteAxisType::ABS_Y, false) => match self.settings.lstick.function.as_str() {
          "cursor" | "scroll" => {
            let axis_value = self.get_axis_value(&event, &self.settings.lstick.deadzone).await;
            let mut lstick_position = self.lstick_position.lock().await;
            lstick_position[event.code() as usize] = axis_value;
          }
          "bind" => {
            let axis_value = self.get_axis_value(&event, &self.settings.lstick.deadzone).await;
            let direction = if axis_value < 0 {
              -1
            } else if axis_value > 0 {
              1
            } else {
              0
            };
            match AbsoluteAxisType(event.code()) {
              AbsoluteAxisType::ABS_Y => match direction {
                -1 if lstick_values.1 != -1 => {
                  self.convert_event(event, Event::Axis(Axis::LSTICK_UP), 1, false).await;
                  lstick_values.1 = -1
                }
                1 if lstick_values.1 != 1 => {
                  self.convert_event(event, Event::Axis(Axis::LSTICK_DOWN), 1, false).await;
                  lstick_values.1 = 1
                }
                0 => {
                  if lstick_values.1 != 0 {
                    match lstick_values.1 {
                      -1 => self.convert_event(event, Event::Axis(Axis::LSTICK_UP), 0, false).await,
                      1 => self.convert_event(event, Event::Axis(Axis::LSTICK_DOWN), 0, false).await,
                      _ => {}
                    }
                    lstick_values.1 = 0;
                  }
                }
                _ => {}
              },
              AbsoluteAxisType::ABS_X => match direction {
                -1 if lstick_values.0 != -1 => {
                  self.convert_event(event, Event::Axis(Axis::LSTICK_LEFT), 1, false).await;
                  lstick_values.0 = -1
                }
                1 => {
                  if lstick_values.0 != 1 {
                    self.convert_event(event, Event::Axis(Axis::LSTICK_RIGHT), 1, false).await;
                    lstick_values.0 = 1
                  }
                }
                0 => {
                  if lstick_values.0 != 0 {
                    match lstick_values.0 {
                      -1 => self.convert_event(event, Event::Axis(Axis::LSTICK_LEFT), 0, false).await,
                      1 => self.convert_event(event, Event::Axis(Axis::LSTICK_RIGHT), 0, false).await,
                      _ => {}
                    }
                    lstick_values.0 = 0;
                  }
                }
                _ => {}
              },
              _ => {}
            }
          }
          _ => {}
        },
        (EventType::ABSOLUTE, _, AbsoluteAxisType::ABS_RX | AbsoluteAxisType::ABS_RY, false) => match self.settings.rstick.function.as_str() {
          "cursor" | "scroll" => {
            let axis_value = self.get_axis_value(&event, &self.settings.rstick.deadzone).await;
            let mut rstick_position = self.rstick_position.lock().await;
            rstick_position[event.code() as usize - 3] = axis_value;
          }
          "bind" => {
            let axis_value = self.get_axis_value(&event, &self.settings.rstick.deadzone).await;
            let direction = if axis_value < 0 {
              -1
            } else if axis_value > 0 {
              1
            } else {
              0
            };
            match AbsoluteAxisType(event.code()) {
              AbsoluteAxisType::ABS_RY => match direction {
                -1 => {
                  if rstick_values.1 != -1 {
                    self.convert_event(event, Event::Axis(Axis::RSTICK_UP), 1, false).await;
                    rstick_values.1 = -1
                  }
                }
                1 => {
                  if rstick_values.1 != 1 {
                    self.convert_event(event, Event::Axis(Axis::RSTICK_DOWN), 1, false).await;
                    rstick_values.1 = 1
                  }
                }
                0 => {
                  if rstick_values.1 != 0 {
                    match rstick_values.1 {
                      -1 => self.convert_event(event, Event::Axis(Axis::RSTICK_UP), 0, false).await,
                      1 => self.convert_event(event, Event::Axis(Axis::RSTICK_DOWN), 0, false).await,
                      _ => {}
                    }
                    rstick_values.1 = 0;
                  }
                }
                _ => {}
              },
              AbsoluteAxisType::ABS_RX => match direction {
                -1 if rstick_values.0 != -1 => {
                  self.convert_event(event, Event::Axis(Axis::RSTICK_LEFT), 1, false).await;
                  rstick_values.0 = -1
                }
                1 => {
                  if rstick_values.0 != 1 {
                    self.convert_event(event, Event::Axis(Axis::RSTICK_RIGHT), 1, false).await;
                    rstick_values.0 = 1
                  }
                }
                0 => {
                  if rstick_values.0 != 0 {
                    match rstick_values.0 {
                      -1 => self.convert_event(event, Event::Axis(Axis::RSTICK_LEFT), 0, false).await,
                      1 => self.convert_event(event, Event::Axis(Axis::RSTICK_RIGHT), 0, false).await,
                      _ => {}
                    }
                    rstick_values.0 = 0;
                  }
                }
                _ => {}
              },
              _ => {}
            }
          }
          _ => {}
        },
        (EventType::ABSOLUTE, _, AbsoluteAxisType::ABS_Z, false) => {
          if !self.settings.stadia {
            match (event.value(), triggers_values.0) {
              (0, 1) => {
                self.convert_event(event, Event::Axis(Axis::BTN_TL2), 0, false).await;
                triggers_values.0 = 0;
              }
              (_, 0) => {
                self.convert_event(event, Event::Axis(Axis::BTN_TL2), 1, false).await;
                triggers_values.0 = 1;
              }
              _ => {}
            }
          } else {
            match self.settings.rstick.function.as_str() {
              "cursor" | "scroll" => {
                let axis_value = self.get_axis_value(&event, &self.settings.rstick.deadzone).await;
                let mut rstick_position = self.rstick_position.lock().await;
                rstick_position[0] = axis_value;
              }
              "bind" => {
                let axis_value = self.get_axis_value(&event, &self.settings.rstick.deadzone).await;
                let direction = if axis_value < 0 {
                  -1
                } else if axis_value > 0 {
                  1
                } else {
                  0
                };
                match direction {
                  -1 if rstick_values.0 != -1 => {
                    self.convert_event(event, Event::Axis(Axis::RSTICK_LEFT), 1, false).await;
                    rstick_values.0 = -1
                  }
                  1 => {
                    if rstick_values.0 != 1 {
                      self.convert_event(event, Event::Axis(Axis::RSTICK_RIGHT), 1, false).await;
                      rstick_values.0 = 1
                    }
                  }
                  0 => {
                    if rstick_values.0 != 0 {
                      match rstick_values.0 {
                        -1 => self.convert_event(event, Event::Axis(Axis::RSTICK_LEFT), 0, false).await,
                        1 => self.convert_event(event, Event::Axis(Axis::RSTICK_RIGHT), 0, false).await,
                        _ => {}
                      }
                      rstick_values.0 = 0;
                    }
                  }
                  _ => {}
                }
              }
              _ => {}
            }
          }
        }
        (EventType::ABSOLUTE, _, AbsoluteAxisType::ABS_RZ, false) => {
          if !self.settings.stadia {
            match (event.value(), triggers_values.1) {
              (0, 1) => {
                self.convert_event(event, Event::Axis(Axis::BTN_TR2), 0, false).await;
                triggers_values.1 = 0;
              }
              (_, 0) => {
                self.convert_event(event, Event::Axis(Axis::BTN_TR2), 1, false).await;
                triggers_values.1 = 1;
              }
              _ => {}
            }
          } else {
            match self.settings.rstick.function.as_str() {
              "cursor" | "scroll" => {
                let axis_value = self.get_axis_value(&event, &self.settings.rstick.deadzone).await;
                let mut rstick_position = self.rstick_position.lock().await;
                rstick_position[1] = axis_value;
              }
              "bind" => {
                let axis_value = self.get_axis_value(&event, &self.settings.rstick.deadzone).await;
                let direction = if axis_value < 0 {
                  -1
                } else if axis_value > 0 {
                  1
                } else {
                  0
                };
                match direction {
                  -1 => {
                    if rstick_values.1 != -1 {
                      self.convert_event(event, Event::Axis(Axis::RSTICK_UP), 1, false).await;
                      rstick_values.1 = -1
                    }
                  }
                  1 => {
                    if rstick_values.1 != 1 {
                      self.convert_event(event, Event::Axis(Axis::RSTICK_DOWN), 1, false).await;
                      rstick_values.1 = 1
                    }
                  }
                  0 => {
                    if rstick_values.1 != 0 {
                      match rstick_values.1 {
                        -1 => self.convert_event(event, Event::Axis(Axis::RSTICK_UP), 0, false).await,
                        1 => self.convert_event(event, Event::Axis(Axis::RSTICK_DOWN), 0, false).await,
                        _ => {}
                      }
                      rstick_values.1 = 0;
                    }
                  }
                  _ => {}
                }
              }
              _ => {}
            }
          }
        }
        (EventType::MISC, _, _, true) => {
          if evdev::MiscType(event.code()) == evdev::MiscType::MSC_SERIAL {
            pen_events.push(event);
            let mut virt_dev = self.virt_dev.lock().await;
            virt_dev.abs.emit(&pen_events).unwrap();
            pen_events.clear()
          }
        }
        _ => self.emit_default_event(event).await,
      }
    }
    let mut device_is_connected = self.device_is_connected.lock().await;
    *device_is_connected = false;

    println!(
      "Disconnected device \"{}\".\n",
      self.current_config.lock().await.name
    );
  }

  async fn convert_event(
    &self,
    default_event: InputEvent,
    event: Event,
    value: i32,
    send_zero: bool,
  ) {
    if value == 1 { self.update_config().await; };

    // Send physical event to Ruby for async processing
    if let Some(ruby) = &self.ruby_service {
      let config = self.current_config.lock().await;
      let modifiers = self.modifiers.lock().await.clone();

      // Check if there's a Ruby script configured for this event
      if let Some(map) = config.bindings.rubies.get(&event) {
        if map.get(&modifiers).is_some() {
          let script = map.get(&modifiers).unwrap();
          println!("Sending event to Ruby: {:?}; event_type: {:?}, code: {}, value: {}; script: {}", event, default_event.event_type(), default_event.code(), value, script);
          let physical_event = crate::magnus_ruby_runtime::PhysicalEvent {
            script: script.to_string(),
            event_type: default_event.event_type().0,
            code: default_event.code(),
            value,
            timestamp_sec: default_event.timestamp().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            timestamp_nsec: default_event.timestamp().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos(),
          };

          let _ = ruby.send_event(physical_event);
          return;
        }
      }
    }

    let config = self.current_config.lock().await;
    let modifiers = self.modifiers.lock().await.clone();

    if let Some(map) = config.bindings.remap.get(&event) {
      if let Some(event_list) = map.get(&modifiers) {
        self.emit_event(
          event_list,
          value,
          &modifiers,
          &config,
          modifiers.is_empty(),
          !modifiers.is_empty(),
        ).await;
        if send_zero {
          let modifiers = self.modifiers.lock().await.clone();
          self.emit_event(
            event_list,
            0,
            &modifiers,
            &config,
            modifiers.is_empty(),
            !modifiers.is_empty(),
          ).await;
        }
        return;
      }

      if let Some(event_list) = map.get(&vec![Event::Hold]) {
        if !modifiers.is_empty() || self.settings.chain_only == false {
          self.emit_event(event_list, value, &modifiers, &config, false, false).await;
          return;
        }
      }

      if let Some(map) = config.bindings.movements.get(&event) {
        if let Some(movement) = map.get(&modifiers) {
          if value <= 1 { self.emit_movement(movement, value).await; }
          return;
        };
      }

      if let Some(event_list) = map.get(&Vec::new()) {
        self.emit_event(event_list, value, &modifiers, &config, true, false).await;
        if send_zero {
          let modifiers = self.modifiers.lock().await.clone();
          self.emit_event(event_list, 0, &modifiers, &config, true, false).await;
        }
        return;
      }
    }

    self.emit_nonmapped_event(default_event, event, value, &modifiers, &config).await;
  }

  async fn emit_event(
    &self,
    event_list: &Vec<Key>,
    value: i32,
    modifiers: &Vec<Event>,
    config: &Config,
    release_keys: bool,
    ignore_modifiers: bool,
  ) {
    let mut virt_dev = self.virt_dev.lock().await;
    let mut modifier_was_activated = self.modifier_was_activated.lock().await;
    if release_keys && value != 2 {
      let released_keys: Vec<Key> = self.released_keys(&modifiers, &config).await;
      for key in released_keys {
        if config.mapped_modifiers.all.contains(&Event::Key(key)) {
          self.toggle_modifiers(Event::Key(key), 0, &config).await;
          let virtual_event: InputEvent = InputEvent::new_now(EventType::KEY, key.code(), 0);
          virt_dev.keys.emit(&[virtual_event]).unwrap();
        }
      }
    } else if ignore_modifiers {
      for key in modifiers.iter() {
        if let Event::Key(key) = key {
          let virtual_event: InputEvent = InputEvent::new_now(EventType::KEY, key.code(), 0);
          virt_dev.keys.emit(&[virtual_event]).unwrap();
        }
      }
    }
    for key in event_list {
      if release_keys && value != 2 {
        self.toggle_modifiers(Event::Key(*key), value, &config).await;
      }
      if config.mapped_modifiers.custom.contains(&Event::Key(*key)) {
        if value == 0 && !*modifier_was_activated {
          let virtual_event: InputEvent = InputEvent::new_now(EventType::KEY, key.code(), 1);
          virt_dev.keys.emit(&[virtual_event]).unwrap();
          let virtual_event: InputEvent = InputEvent::new_now(EventType::KEY, key.code(), 0);
          virt_dev.keys.emit(&[virtual_event]).unwrap();
          *modifier_was_activated = true;
        } else if value == 1 {
          *modifier_was_activated = false;
        }
      } else {
        let virtual_event: InputEvent = InputEvent::new_now(EventType::KEY, key.code(), value);
        virt_dev.keys.emit(&[virtual_event]).unwrap();
        *modifier_was_activated = true;
      }
    }
  }

  async fn emit_nonmapped_event(
    &self,
    default_event: InputEvent,
    event: Event,
    value: i32,
    modifiers: &Vec<Event>,
    config: &Config,
  ) {
    let mut virt_dev = self.virt_dev.lock().await;
    let mut modifier_was_activated = self.modifier_was_activated.lock().await;
    if config.mapped_modifiers.all.contains(&event) && value != 2 {
      let released_keys: Vec<Key> = self.released_keys(&modifiers, &config).await;
      for key in released_keys {
        self.toggle_modifiers(Event::Key(key), 0, &config).await;
        let virtual_event: InputEvent = InputEvent::new_now(EventType::KEY, key.code(), 0);
        virt_dev.keys.emit(&[virtual_event]).unwrap()
      }
    }
    self.toggle_modifiers(event, value, &config).await;
    if config.mapped_modifiers.custom.contains(&event) {
      if value == 0 && !*modifier_was_activated {
        let virtual_event: InputEvent = InputEvent::new_now(default_event.event_type(), default_event.code(), 1);
        virt_dev.keys.emit(&[virtual_event]).unwrap();
        let virtual_event: InputEvent = InputEvent::new_now(default_event.event_type(), default_event.code(), 0);
        virt_dev.keys.emit(&[virtual_event]).unwrap();
        *modifier_was_activated = true;
      } else if value == 1 {
        *modifier_was_activated = false;
      }
    } else {
      *modifier_was_activated = true;
      match default_event.event_type() {
        EventType::KEY => virt_dev.keys.emit(&[default_event]).unwrap(),
        EventType::RELATIVE => virt_dev.axis.emit(&[default_event]).unwrap(),
        EventType::ABSOLUTE => virt_dev.abs.emit(&[default_event]).unwrap(),
        EventType::MISC => self.virt_dev.lock().await.abs.emit(&[default_event]).unwrap(),
        _ => {}
      }
    }
  }

  async fn emit_default_event(&self, event: InputEvent) {
    match event.event_type() {
      EventType::KEY => self.virt_dev.lock().await.keys.emit(&[event]).unwrap(),
      EventType::RELATIVE => self.virt_dev.lock().await.axis.emit(&[event]).unwrap(),
      EventType::ABSOLUTE => self.virt_dev.lock().await.abs.emit(&[event]).unwrap(),
      EventType::MISC => self.virt_dev.lock().await.abs.emit(&[event]).unwrap(),
      _ => {}
    }
  }

  async fn emit_movement(&self, movement: &Relative, value: i32) {
    let mut cursor_movement = self.cursor_movement.lock().await;
    let mut scroll_movement = self.scroll_movement.lock().await;
    match movement {
      Relative::Cursor(Cursor::CURSOR_UP) => cursor_movement.1 = -value,
      Relative::Cursor(Cursor::CURSOR_DOWN) => cursor_movement.1 = value,
      Relative::Cursor(Cursor::CURSOR_LEFT) => cursor_movement.0 = -value,
      Relative::Cursor(Cursor::CURSOR_RIGHT) => cursor_movement.0 = value,
      Relative::Scroll(Scroll::SCROLL_UP) => scroll_movement.1 = -value,
      Relative::Scroll(Scroll::SCROLL_DOWN) => scroll_movement.1 = value,
      Relative::Scroll(Scroll::SCROLL_LEFT) => scroll_movement.0 = -value,
      Relative::Scroll(Scroll::SCROLL_RIGHT) => scroll_movement.0 = value,
    };
  }

  async fn spawn_subprocess(&self, command_list: &Vec<String>) {
    let mut modifier_was_activated = self.modifier_was_activated.lock().await;
    *modifier_was_activated = true;
    let (user, running_as_root) = if let Ok(sudo_user) = &self.environment.sudo_user {
      (Some(sudo_user), true)
    } else if let Ok(user) = &self.environment.user {
      (Some(user), false)
    } else {
      (None, false)
    };
    if let Some(user) = user {
      for command in command_list {
        if running_as_root {
          match fork() {
            Ok(Fork::Child) => match fork() {
              Ok(Fork::Child) => {
                setsid().unwrap();
                Command::new("runuser")
                  .args([user, "-c", command])
                  .stdin(Stdio::null())
                  .stdout(Stdio::null())
                  .stderr(Stdio::null())
                  .spawn()
                  .unwrap();
                std::process::exit(0);
              }
              Ok(Fork::Parent(_)) => std::process::exit(0),
              Err(_) => std::process::exit(1),
            },
            Ok(Fork::Parent(_)) => (),
            Err(_) => std::process::exit(1),
          }
        } else {
          Command::new("sh")
            .arg("-c")
            .arg(format!("systemd-run --user -M {}@ {}", user, command))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        }
      }
    }
  }

  async fn get_axis_value(&self, event: &InputEvent, deadzone: &i32) -> i32 {
    let distance_from_center: i32 = match self.settings.axis_16_bit {
      false => (event.value() - 128) * 200,
      _ => event.value(),
    };
    if distance_from_center.abs() <= deadzone * 200 {
      0
    } else {
      (distance_from_center + 2000 - 1) / 2000
    }
  }

  async fn toggle_modifiers(&self, modifier: Event, value: i32, config: &Config) {
    let mut modifiers = self.modifiers.lock().await;
    if config.mapped_modifiers.all.contains(&modifier) {
      match value {
        1 => {
          modifiers.push(modifier);
          modifiers.sort();
          modifiers.dedup();
        }
        0 => modifiers.retain(|&x| x != modifier),
        _ => {}
      }
    }
  }

  async fn released_keys(&self, modifiers: &Vec<Event>, config: &Config) -> Vec<Key> {
    let mut released_keys: Vec<Key> = Vec::new();
    for (_key, hashmap) in config.bindings.remap.iter() {
      if let Some(event_list) = hashmap.get(modifiers) {
        released_keys.extend(event_list);
      }
    }
    released_keys
  }

  async fn change_active_layout(&self) {
    let mut active_layout = self.active_layout.lock().await;
    let active_window = get_active_window(&self.environment, &self.config).await;
    loop {
      if *active_layout == 3 {
        *active_layout = 0
      } else {
        *active_layout += 1
      };
      if let Some(_) = self.config.iter().find(|&x| {
        x.associations.layout == *active_layout && x.associations.client == active_window
      }) {
        break;
      };
    }
    if self.settings.notify_layout_switch {
      let notify = vec![String::from(format!(
        "notify-send -t 500 'Makita' 'Switching to layout {}'",
        *active_layout
      ))];
      self.spawn_subprocess(&notify).await;
    }
  }

  fn update_config(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
    Box::pin(async move {
      let active_layout = self.active_layout.lock().await.clone();
      let active_window = get_active_window(&self.environment, &self.config).await;
      let associations = Associations {
        client: active_window,
        layout: active_layout,
      };
      match self.config.iter().find(|&x| x.associations == associations) {
        Some(config) => {
          let mut current_config = self.current_config.lock().await;
          *current_config = config.clone();
        }
        None => {
          self.change_active_layout().await;
          self.update_config().await;
        }
      };
    })
  }

  async fn loop_2d(&self, subject: &str, invert_axis: bool, event_x_id: u16, event_y_id: u16) {
    let (direction, sensitivity, activation_modifiers) =
      if self.settings.lstick.function.as_str() == subject {
        ("left", self.settings.lstick.sensitivity, &self.settings.lstick.activation_modifiers)
      } else if self.settings.rstick.function.as_str() == subject {
        ("right", self.settings.rstick.sensitivity, &self.settings.rstick.activation_modifiers)
      } else {
        ("disabled", 0, &vec![])
      };

    if sensitivity != 0 {
      while *self.device_is_connected.lock().await {
        let stick_position = if direction == "left" {
          self.lstick_position.lock().await
        } else if direction == "right" {
          self.rstick_position.lock().await
        } else {
          break;
        };
        if stick_position[0] != 0 || stick_position[1] != 0 {
          let modifiers = self.modifiers.lock().await;
          if activation_modifiers.len() == 0 || *activation_modifiers == *modifiers {
            let (x_coord, y_coord) = if invert_axis {
              (-stick_position[0], -stick_position[1])
            } else {
              (stick_position[0], stick_position[1])
            };
            let virtual_event_x: InputEvent = InputEvent::new_now(EventType::RELATIVE, event_x_id, x_coord);
            let virtual_event_y: InputEvent = InputEvent::new_now(EventType::RELATIVE, event_y_id, y_coord);
            let mut virt_dev = self.virt_dev.lock().await;
            virt_dev.axis.emit(&[virtual_event_x]).unwrap();
            virt_dev.axis.emit(&[virtual_event_y]).unwrap();
          }
        }
        tokio::time::sleep(std::time::Duration::from_millis(sensitivity)).await;
      }
    }
  }

  async fn key_loop_2d(&self, subject_settings: &Movement, movement: &Arc<Mutex<(i32, i32)>>, event_x_id: u16, event_y_id: u16) {
    let (speed, acceleration, mut current_speed) = (
      if subject_settings.speed == 0 {
        return;
      } else {
        subject_settings.speed
      },
      if subject_settings.acceleration.abs() > 1.0 {
        1.0
      } else {
        subject_settings.acceleration.abs()
      },
      subject_settings.speed as f32,
    );

    while *self.device_is_connected.lock().await {
      let locked_movement = movement.lock().await;
      if *locked_movement == (0, 0) {
        current_speed = 0.0
      } else {
        current_speed += speed as f32 * acceleration / 10.0;
        if current_speed > speed as f32 {
          current_speed = speed as f32
        }
        if locked_movement.0 != 0 {
          let mut virt_dev = self.virt_dev.lock().await;
          let virtual_event_x: InputEvent = InputEvent::new_now(EventType::RELATIVE, event_x_id, locked_movement.0 * current_speed as i32);
          virt_dev.axis.emit(&[virtual_event_x]).unwrap();
        }
        if locked_movement.1 != 0 {
          let mut virt_dev = self.virt_dev.lock().await;
          let virtual_event_y: InputEvent = InputEvent::new_now(EventType::RELATIVE, event_y_id, locked_movement.1 * current_speed as i32);
          virt_dev.axis.emit(&[virtual_event_y]).unwrap();
        }
      }
    }

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
  }
}
