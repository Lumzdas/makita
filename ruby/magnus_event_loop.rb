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
    end
  end

  def handle_event(event_data)
  end

  def start_event_loop
    makita_log("info", "Starting Magnus-based event loop")

    Fiber.set_scheduler(FiberScheduler.new)
    Fiber.schedule do
      while true
        events_data = makita_get_events

        events_data.each do |event_data|
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

  def key_down?
    @value == 1
  end

  def key_up?
    @value == 0
  end

  def key_hold?
    @value == 2
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
  KB_LALT = 56
  KB_LTAB = 15
  KB_ESC = 1
  KB_ENTER = 28
  KB_A = 30
  KB_B = 48

  class << self
    def runtime
      @runtime ||= Thread.current[:makita_runtime]
    end

    def press(key_code)
      send_synthetic_event(1, key_code, 1)
      send_synthetic_event(1, key_code, 0)
    end

    def press_down(*key_codes)
      key_codes.each do |key_code|
        send_synthetic_event(1, key_code, 1)
      end
    end

    def release(key_code)
      send_synthetic_event(1, key_code, 0)
    end

    def get_key_state(key_code)
      result = makita_query_state("KeyState", key_code)
      result == "true"
    end

    def get_modifier_state
      result = makita_query_state("ModifierState", nil)
      begin
        result.gsub(/[\[\]]/, '').split(',').map(&:to_i)
      rescue
        []
      end
    end

    def device_connected?
      result = makita_query_state("DeviceConnected", nil)
      result == "true"
    end

    private

    def send_synthetic_event(event_type, code, value)
      makita_send_synthetic_event(event_type, code, value)
    end
  end
end

# Initialize global runtime instance
# This will be created from Rust side, so we don't run it automatically
