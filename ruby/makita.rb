module Makita
  # space for storing variables between scripts
  @@stuff = {}
  def self.stuff = @@stuff

  KEY_VALUE_UP = 0
  KEY_VALUE_DOWN = 1
  KEY_VALUE_HOLD = 2

  # EVENT_TYPE_KEY = defined back in Rust
  # EVENT_TYPE_RELATIVE = defined back in Rust
  # EVENT_TYPE_ABSOLUTE = defined back in Rust
  # EVENT_TYPE_SWITCH = defined back in Rust
  # EVENT_TYPE_LED = defined back in Rust
  # EVENT_TYPE_SOUND = defined back in Rust
  # EVENT_TYPE_FORCEFEEDBACKSTATUS = defined back in Rust

  class << self
    def press(key_code)
      send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_DOWN)
      yield if block_given?
    ensure
      send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_UP)
    end

    def press_down(*key_codes)
      key_codes.each do |key_code|
        send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_DOWN)
      end
    end

    def release(key_code)
      send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_UP)
    end

    def pass(event)
      send_synthetic_event(event.event_type, event.code, event.value)
    end

    def type_text(string, delay_seconds: 0)
      string.each_char do |char|
        case char_to_keycode(char)
        in [key_code, :lower]
          press(key_code)
          sleep(delay_seconds) if delay_seconds > 0
        in [key_code, :upper]
          press(KEY_LEFTSHIFT) { press(key_code) }
          sleep(delay_seconds) if delay_seconds > 0
        else
          makita_log("warn", "No keycode mapping for character: #{char}")
        end
      end
    end

    private

    def send_synthetic_event(event_type, code, value)
      makita_send_synthetic_event(event_type, code, value)
    end

    def char_to_keycode(char)
      case char
      in /[a-z0-9]/
        [const_get("KEY_" + char.upcase), :lower]
      in /[A-Z]/
        [const_get("KEY_" + char), :upper]
      else
        CHAR_TO_KEYCODE[char]
      end
    end
  end
end
