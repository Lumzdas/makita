#!/usr/bin/env ruby

require 'json'
require 'fiber'

class MagnusRuntime
  def initialize
    @scripts = {}
  end

  def load_script(name, path)
    begin
      content = File.read(path)
      @scripts[name] = content
      makita_log("info", "Script loaded: #{name}")
    rescue => e
      makita_log("error", "Failed to load script #{name}: #{e.message}")
      makita_log("error", "    from #{e.backtrace.first}")
      raise
    end
  end

  def handle_event(event_data)
  end

  def start_event_loop
    makita_log("info", "Starting Magnus-based event loop")

    Fiber.set_scheduler(FiberScheduler.new)
    Fiber.schedule do
      while true
        makita_get_events.each do |event_data|
          script_name = event_data['script']
          if script = @scripts[script_name]
            event = Event.new(event_data)
            Fiber.schedule do
              eval(script)
              makita_log("debug", "Event processed by script: #{script_name}")
            rescue => e
              makita_log("error", "Event processing error in #{script_name}: #{e.message}")
              makita_log("error", "    from #{e.backtrace.first}")
            end
          else
            makita_log("error", "Script not loaded: #{script_name}")
          end
        end

        sleep 0.001
      end
    end

    Fiber.scheduler.run
  end
end

class Event
  def initialize(data)
    @event_type = data['event_type']
    @code = data['code']
    @value = data['value']
    @timestamp_sec = data['timestamp_sec']
    @timestamp_nsec = data['timestamp_nsec']
    @script = data['script']
  end

  def key
    @code == 0 ? nil : @code
  end

  def key_up?
    @value == Makita::KEY_VALUE_UP
  end

  def key_down?
    @value == Makita::KEY_VALUE_DOWN
  end

  def key_hold?
    @value == Makita::KEY_VALUE_HOLD
  end

  def event_type
    @event_type
  end

  def code
    @code
  end

  def value
    @value
  end

  def script
    @script
  end

  def to_s
    "Event(type=#{@event_type}, code=#{@code}, value=#{@value}, time=#{@timestamp_sec}.#{@timestamp_nsec}, script=#{@script})"
  end
end

module Makita
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
    def runtime
      @runtime ||= Thread.current[:makita_runtime]
    end

    def press(key_code)
      send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_DOWN)
      send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_UP)
    end

    def press_down(*key_codes)
      key_codes.each do |key_code|
        send_synthetic_event(EVENT_TYPE_KEY, key_code, KEY_VALUE_DOWN)
      end
    end

    def release(key_code)
      send_synthetic_event(1, key_code, KEY_VALUE_UP)
    end

    def get_key_state(key_code)
      makita_query_state("KeyState", key_code) == "true"
    end

    private

    def send_synthetic_event(event_type, code, value)
      makita_send_synthetic_event(event_type, code, value)
    end
  end
end

# Initialize global runtime instance
# This will be created from Rust side, so we don't run it automatically
