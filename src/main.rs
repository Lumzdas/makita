mod active_client;
mod config;
mod ruby_runtime;
mod udev_monitor;
mod virtual_devices;
mod input_event_handling;

use crate::udev_monitor::*;
use config::Config;
use std::env;
use std::sync::Arc;
use tokio;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use crate::ruby_runtime::RubyService;

#[tokio::main]
async fn main() {
  let config_directory = match env::var("MAKITA_CONFIG") {
    Ok(directory) => {
      println!("MAKITA_CONFIG set to {}.", directory);
      directory
    }
    Err(_) => {
      let user_home = match env::var("HOME") {
        Ok(user_home) if user_home == "/root".to_string() => match env::var("SUDO_USER") {
          Ok(sudo_user) => format!("/home/{}", sudo_user),
          _ => user_home,
        },
        Ok(user_home) => user_home,
        _ => "/root".to_string(),
      };
      let directory = format!("{}/.config/makita", user_home);
      println!("MAKITA_CONFIG environment variable is not set, defaulting to {}.", directory);
      directory
    }
  };

  let mut configs: Vec<Config> = Vec::new();
  match std::fs::read_dir(config_directory.clone()) {
    Ok(directory_iterator) => {
      for file in directory_iterator {
        let filename: String = file.as_ref().unwrap().file_name().into_string().unwrap();

        if filename.ends_with(".toml") && !filename.starts_with(".") {
          let name: String = filename.split(".toml").collect::<Vec<&str>>()[0].to_string();
          let config_file: Config = Config::new_from_file(file.unwrap().path().to_str().unwrap(), name);
          configs.push(config_file);
        }
      }
    },
    _ => {
      println!("Config directory not found, exiting Makita.");
      std::process::exit(1);
    }
  }

  let ruby_scripts_directory = match env::var("MAKITA_RUBY_SCRIPTS") {
    Ok(directory) => directory,
    _ => {
      let directory = format!("{}/{}", config_directory, "scripts");
      println!("MAKITA_RUBY_SCRIPTS environment variable is not set, defaulting to {}", directory);
      directory
    }
  };

  let mut rubies = Vec::new();
  for config in configs.clone() {
    for (_event, modifier_map) in config.bindings.rubies {
      for (_modifiers, script_name) in modifier_map {
        let script_path = format!("{}/{}.rb", ruby_scripts_directory, script_name);
        rubies.push((script_name, script_path));
      }
    }
  }

  let ruby_service = start_ruby_service(rubies);

  // if ruby_service.is_some() {
  //   println!("[UdevMonitor] Creating EventSender for {}...", device.0.to_str().unwrap());
  //   let event_sender = EventSender::new(ruby_service, virt_dev.clone());
  //   tasks.push(tokio::spawn(start_event_sender(event_sender)));
  // }

  let tasks: Vec<JoinHandle<()>> = Vec::new();
  start_monitoring_udev(configs, tasks, ruby_service).await;
}

fn start_ruby_service(rubies: Vec<(String, String)>) -> Option<Arc<Mutex<RubyService>>> {
  if rubies.is_empty() { return None }

  println!("Initializing Ruby service...");
  let service = RubyService::new(move |query| {
    use crate::ruby_runtime::{StateQuery, StateResponse};
    match query {
      StateQuery::KeyState(_key_code) => {
        // TODO: implement
        StateResponse::KeyState(false)
      }
    }
  }).expect("Failed to create Ruby service");

  for ruby in rubies {
    println!("Loading Ruby script: {}", ruby.0);
    let _ = service.load_script(ruby.0, ruby.1);
  }

  println!("Starting Ruby event loop...");
  service.start_event_loop().expect("Failed to start Ruby event loop");
  println!("Ruby service initialized.");
  Some(Arc::new(Mutex::new(service)))
}
