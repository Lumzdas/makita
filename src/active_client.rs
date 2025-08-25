use crate::udev_monitor::{Client, Environment, Server};
use crate::Config;
use serde_json;
use std::process::{Command, Stdio};
use swayipc_async::Connection;
use x11rb::protocol::xproto::{get_input_focus, get_property, Atom, AtomEnum};

pub async fn get_active_window(environment: &Environment, config: &Vec<Config>) -> Client {
  match &environment.server {
    Server::Connected(server) => {
      match server.as_str() {
        "Hyprland" => {
          let query = Command::new("hyprctl").args(["activewindow", "-j"]).output().unwrap();
          if let Ok(reply) = serde_json::from_str::<serde_json::Value>(std::str::from_utf8(query.stdout.as_slice()).unwrap()) {
            match_window(config, Client::Class(reply["class"].to_string().replace("\"", "")))
          } else {
            Client::Default
          }
        }

        "sway" => {
          let mut connection = Connection::new().await.unwrap();
          let active_window = match connection.get_tree().await.unwrap().find_focused(|window| window.focused) {
            Some(window) => match window.app_id {
              Some(id) => Client::Class(id),
              None => window.window_properties.and_then(|window_properties| window_properties.class).map_or(Client::Default, Client::Class),
            },
            None => Client::Default,
          };

          match_window(config, active_window)
        }

        "niri" => {
          let query = Command::new("niri").args(["msg", "-j", "focused-window"]).output().unwrap();
          if let Ok(reply) = serde_json::from_str::<serde_json::Value>(std::str::from_utf8(query.stdout.as_slice()).unwrap()) {
            match_window(config, Client::Class(reply["app_id"].to_string().replace("\"", "")))
          } else {
            Client::Default
          }
        }

        "KDE" => {
          let (user, running_as_root) =
            if let Ok(sudo_user) = environment.sudo_user.clone() {
              (Some(sudo_user), true)
            } else if let Ok(user) = environment.user.clone() {
              (Some(user), false)
            } else {
              (None, false)
            };

          let active_window = {
            if let Some(user) = user {
              let output = if running_as_root {
                let command = "kdotool getactivewindow getwindowclassname";
                Command::new("runuser").arg(user).arg("-c").arg(command).output().unwrap()
              } else {
                let command = format!("systemd-run --user --scope -M {}@ kdotool getactivewindow getwindowclassname", user);
                Command::new("sh").arg("-c").arg(command).stderr(Stdio::null()).output().unwrap()
              };
              Client::Class(std::str::from_utf8(output.stdout.as_slice()).unwrap().trim().to_string())
            } else {
              Client::Default
            }
          };

          match_window(config, active_window)
        }

        "x11" => {
          let connection = x11rb::connect(None).unwrap().0;
          let focused_window = get_input_focus(&connection).unwrap().reply().unwrap().focus;
          let (wm_class, string): (Atom, Atom) = (AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());
          let class = get_property(&connection, false, focused_window, wm_class, string, 0, u32::MAX)
            .unwrap()
            .reply()
            .unwrap()
            .value;

          if let Some(middle) = class.iter().position(|&byte| byte == 0) {
            let mut class = &class.split_at(middle).1[1..];
            if class.last() == Some(&0) { class = &class[..class.len() - 1]; }

            match_window(config, Client::Class(std::str::from_utf8(class).unwrap().to_string()))
          } else {
            Client::Default
          }
        }
        _ => Client::Default,
      }
    }
    Server::Unsupported => Client::Default,
    Server::Failed => Client::Default,
  }
}

fn match_window(config: &Vec<Config>, active_window: Client) -> Client {
  if let Some(_) = config.iter().find(|&x| x.associations.client == active_window) {
    active_window
  } else {
    Client::Default
  }
}
