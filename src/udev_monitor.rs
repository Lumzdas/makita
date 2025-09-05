use crate::config::{Associations, Event};
use crate::input_event_handling::event_reader::EventReader;
use crate::input_event_handling::event_sender::EventSender;
use crate::virtual_devices::VirtualDevices;
use crate::Config;
use evdev::{Device, EventStream};
use std::{env, path::Path, process::Command, sync::Arc};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tokio::signal;

#[derive(Debug, Default, Eq, PartialEq, Hash, Clone)]
pub enum Client {
  #[default]
  Default,
  Class(String),
}

#[derive(Clone)]
pub enum Server {
  Connected(String),
  Unsupported,
  Failed,
}

#[derive(Clone)]
pub struct Environment {
  pub user: Result<String, env::VarError>,
  pub sudo_user: Result<String, env::VarError>,
  pub server: Server,
}

pub async fn start_monitoring_udev(config_files: Vec<Config>, mut tasks: Vec<JoinHandle<()>>) {
  let environment = set_environment();
  launch_tasks(&config_files, &mut tasks, environment.clone());

  let mut monitor = tokio_udev::AsyncMonitorSocket::new(
    tokio_udev::MonitorBuilder::new()
      .unwrap()
      .match_subsystem(std::ffi::OsStr::new("input"))
      .unwrap()
      .listen()
      .unwrap(),
  ).unwrap();

  let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt()).expect("Failed to register SIGINT handler");

  loop {
    tokio::select! {
      // Handle udev events
      event = monitor.next() => {
        match event {
          Some(Ok(event)) => {
            if is_mapped(&event.device(), &config_files) {
              println!("[UdevMonitor] Reinitializing...");
              for task in &tasks { task.abort(); }
              tasks.clear();
              launch_tasks(&config_files, &mut tasks, environment.clone())
            }
          }
          Some(Err(e)) => {
            eprintln!("[UdevMonitor] Udev monitor error: {}", e);
          }
          None => {
            println!("[UdevMonitor] Udev monitor ended");
            break;
          }
        }
      }

      _ = sigint.recv() => {
        println!("[UdevMonitor] Received SIGINT, shutting down...");
        for task in tasks.drain(..) { task.abort(); }

        println!("[UdevMonitor] All tasks stopped. Exiting...");
        break;
      }
    }
  }
}

pub fn launch_tasks(
  config_files: &Vec<Config>,
  tasks: &mut Vec<JoinHandle<()>>,
  environment: Environment,
) {
  let modifiers: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Default::default()));
  let modifier_was_activated: Arc<Mutex<bool>> = Arc::new(Mutex::new(true));
  let user_has_access = match Command::new("groups").output() {
    Ok(groups) if std::str::from_utf8(&groups.stdout.as_slice()).unwrap().contains("input") => {
      println!("[UdevMonitor] Evdev permissions available. Scanning for event devices with a matching config file...");
      true
    },
    Ok(groups) if std::str::from_utf8(&groups.stdout.as_slice()).unwrap().contains("root") => {
      println!("[UdevMonitor] Root permissions available. Scanning for event devices with a matching config file...");
      true
    },
    Ok(_) => {
      println!("[UdevMonitor] Warning: user has no access to event devices, Makita might not be able to detect all connected devices. \
                Note: Run Makita with 'sudo -E makita' or as a system service. Refer to the docs for more info. Continuing...");
      false
    },
    Err(_) => {
      println!("[UdevMonitor] Warning: unable to determine if user has access to event devices. Continuing...");
      false
    }
  };

  let devices: evdev::EnumerateDevices = evdev::enumerate();
  let mut devices_found = 0;
  for device in devices {
    let mut config_list: Vec<Config> = Vec::new();
    for mut config in config_files.clone() {
      let split_config_name = config.name.split("::").collect::<Vec<&str>>();
      let associated_device_name = split_config_name[0];

      if associated_device_name == device.1.name().unwrap().replace("/", "") {
        let (window_class, layout) = match split_config_name.len() {
          1 => (Client::Default, 0),
          2 => {
            if let Ok(layout) = split_config_name[1].parse::<u16>() {
              (Client::Default, layout)
            } else {
              (Client::Class(split_config_name[1].to_string()), 0)
            }
          }
          3 => {
            if let Ok(layout) = split_config_name[1].parse::<u16>() {
              (Client::Class(split_config_name[2].to_string()), layout)
            } else if let Ok(layout) = split_config_name[2].parse::<u16>() {
              (Client::Class(split_config_name[1].to_string()), layout)
            } else {
              println!("[UdevMonitor] Warning: unable to parse layout number in {}, treating it as default.", config.name);
              (Client::Default, 0)
            }
          }
          _ => {
            println!("[UdevMonitor] Warning: too many arguments in config file name {}, treating it as default.", config.name);
            (Client::Default, 0)
          }
        };

        config.associations.client = window_class;
        config.associations.layout = layout;
        config_list.push(config.clone());
      };
    }

    if config_list.len() > 0 && !config_list.iter().any(|x| x.associations == Associations::default()) {
      config_list.push(Config::new_empty(device.1.name().unwrap().replace("/", "")));
    }

    let event_device = device.0.as_path().to_str().unwrap().to_string();
    if config_list.len() != 0 {
      if device.0.to_str().unwrap() == "/dev/input/event7" { // TODO: move ruby runtime creation to main.rs so that it is created only once
        let stream = Arc::new(Mutex::new(get_event_stream(
          Path::new(&event_device),
          config_list.clone(),
        )));
        println!("[UdevMonitor] Constructing reader for {} ({})...", device.0.to_str().unwrap(), device.1.name().unwrap());
        let virt_dev = Arc::new(Mutex::new(VirtualDevices::new(device.1)));
        let reader = EventReader::new(
          config_list.clone(),
          virt_dev.clone(),
          stream,
          modifiers.clone(),
          modifier_was_activated.clone(),
          environment.clone(),
        );

        if let Some(ruby_service) = reader.get_ruby_service() {
          println!("[UdevMonitor] Creating EventSender for {}...", device.0.to_str().unwrap());
          let event_sender = EventSender::new(ruby_service, virt_dev.clone());
          tasks.push(tokio::spawn(start_event_sender(event_sender)));
        }

        tasks.push(tokio::spawn(start_reader(reader)));
        devices_found += 1;
      }
    }
  }

  if devices_found == 0 && !user_has_access {
    println!("[UdevMonitor] No matching devices found. Note: make sure that your user has access to event devices.");
  } else if devices_found == 0 && user_has_access {
    println!("[UdevMonitor] No matching devices found. Note: double-check that your device and its associated config file have the same name, as reported by 'evtest'.");
  }
}

pub async fn start_reader(reader: EventReader) {
  reader.start().await;
}

pub async fn start_event_sender(event_sender: EventSender) {
  if let Err(e) = event_sender.start().await {
    eprintln!("[UdevMonitor] EventSender error: {}", e);
  }
}

fn set_environment() -> Environment {
  match env::var("DBUS_SESSION_BUS_ADDRESS") {
    Ok(_) => copy_variables(),
    Err(_) => {
      let uid = Command::new("sh").arg("-c").arg("id -u").output().unwrap();
      let uid_number = std::str::from_utf8(uid.stdout.as_slice()).unwrap().trim();
      if uid_number != "0" {
        let bus_address = format!("unix:path=/run/user/{}/bus", uid_number);
        env::set_var("DBUS_SESSION_BUS_ADDRESS", bus_address);
        copy_variables()
      } else {
        println!("[UdevMonitor] Warning: unable to inherit user environment. \
                  Launch Makita with 'sudo -E makita' or make sure that your systemd unit is running with the 'User=<username>' parameter.");
      }
    }
  };

  if let (Err(env::VarError::NotPresent), Ok(_)) = (env::var("XDG_SESSION_TYPE"), env::var("WAYLAND_DISPLAY")) {
    env::set_var("XDG_SESSION_TYPE", "wayland")
  }

  let supported_compositors = vec!["Hyprland", "sway", "KDE", "niri"]
    .into_iter()
    .map(|str| String::from(str))
    .collect::<Vec<String>>();
  let (x11, wayland) = (String::from("x11"), String::from("wayland"));

  let server: Server = match (env::var("XDG_SESSION_TYPE"), env::var("XDG_CURRENT_DESKTOP")) {
    (Ok(session), Ok(desktop)) if session == wayland && supported_compositors.contains(&desktop) => {
      let server = 'a: {
        if desktop == String::from("KDE") {
          if let Err(_) = Command::new("kdotool").output() {
            println!(
              "[UdevMonitor] Running on KDE but kdotool doesn't seem to be installed. \
               Won't be able to change bindings according to the active window."
            );
            break 'a Server::Unsupported;
          }
        }
        println!("[UdevMonitor] Running on {}, per application bindings enabled.", desktop);
        Server::Connected(desktop.clone())
      };
      server
    },
    (Ok(session), Ok(desktop)) if session == wayland => {
      println!("[UdevMonitor] Warning: unsupported compositor: {}, won't be able to change bindings according to the active window. \
                Currently supported desktops: Hyprland, Sway, Niri, Plasma/KWin, X11.", desktop);
      Server::Unsupported
    },
    (Ok(session), _) if session == x11 => {
      println!("[UdevMonitor] Running on X11, per application bindings enabled.");
      Server::Connected(session)
    },
    (Ok(session), Err(_)) if session == wayland => {
      println!("[UdevMonitor] Warning: unable to retrieve the current desktop based on XDG_CURRENT_DESKTOP env var. \
                Won't be able to change bindings according to the active window.");
      Server::Unsupported
    },
    (Err(_), _) => {
      println!("[UdevMonitor] Warning: unable to retrieve the session type based on XDG_SESSION_TYPE or WAYLAND_DISPLAY env vars. \
                Is your Wayland compositor or X server running? \
                Exiting Makita.");
      std::process::exit(0);
    },
    _ => Server::Failed,
  };

  Environment {
    user: env::var("USER"),
    sudo_user: env::var("SUDO_USER"),
    server,
  }
}

fn copy_variables() {
  let command = Command::new("sh").arg("-c").arg("systemctl --user show-environment").output().unwrap();
  let vars = std::str::from_utf8(command.stdout.as_slice()).unwrap().split("").collect::<Vec<&str>>();

  for var in vars {
    if let Some((variable, value)) = var.split_once("=") {
      if let Err(env::VarError::NotPresent) = env::var(variable) {
        env::set_var(variable, value);
      } else if variable == "PATH" {
        env::set_var("PATH", format!("{}:{}", value, env::var("PATH").unwrap()));
      }
    }
  }
}

pub fn get_event_stream(path: &Path, config: Vec<Config>) -> EventStream {
  let mut device: Device = Device::open(path).expect("Couldn't open device path.");
  match config.iter().find(|&x| x.associations == Associations::default()).unwrap().settings.get("GRAB_DEVICE") {
    Some(value) => {
      if value == &true.to_string() {
        device.grab().expect("Unable to grab device. Is another instance of Makita running?")
      }
    }
    None => device.grab().expect("Unable to grab device. Is another instance of Makita running?"),
  }

  device.into_event_stream().unwrap()
}

pub fn is_mapped(udev_device: &tokio_udev::Device, config_files: &Vec<Config>) -> bool {
  match udev_device.devnode() {
    Some(devnode) => {
      let evdev_devices: evdev::EnumerateDevices = evdev::enumerate();
      for evdev_device in evdev_devices {
        for config in config_files {
          if config.name.contains(&evdev_device.1.name().unwrap().to_string().replace("/", "")) && devnode.to_path_buf() == evdev_device.0 {
            return true;
          }
        }
      }
    }
    _ => return false,
  }

  false
}
