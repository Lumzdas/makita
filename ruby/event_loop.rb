#!/usr/bin/env ruby

require 'json'
require 'fiber'

require_relative 'fiber_scheduler/fiber_scheduler'

def send_to_makita(*strings)
  strings.each { puts _1 }
  $stdout.flush
end

def print_error(message, error)
  send_to_makita(
    "ERROR:#{message}",
    "ERROR:    #{error.message}",
    "ERROR:    from #{error.backtrace.first}"
  )
end

class MakitaRuntime
  def initialize
    @scripts = {}
    @synthetic_events = []
    @state_queries = []
  end

  def load_script(name, path)
    begin
      content = File.read(path)
      @scripts[name] = content
      send_to_makita("LOADED:#{name}")
    rescue => e
      print_error("Failed to load script #{name}", e)
    end
  end

  def handle_event(event_data, script_name)
    Fiber.schedule do
      event = Event.new(event_data)
      eval(@scripts[script_name])
    end
  end

  def run
    send_to_makita("READY")

    handlers = {
      'LOAD' => ->(content) {
        parts = content.split(":", 2)
        load_script(parts[0], parts[1]) if parts.length == 2
      },
      'EVENT' => ->(content) {
        begin
          event_data = JSON.parse(content)
          handle_event(event_data, event_data['script'])
        rescue JSON::ParserError => e
          print_error("Invalid JSON", e)
        end
      }
    }

    FiberScheduler do
      Fiber.schedule do
        while true
          if IO.select([STDIN], nil, nil, 0.001) # 1ms timeout
            begin
              line = STDIN.gets
              break if line.nil? # EOF

              stripped = line.strip
              next if stripped.empty?

              command, content = stripped.split(':', 2)
              handlers[command]&.call(content)
            rescue => e
              print_error("Input processing error", e)
            end
          end

          sleep(0.001)
        end
      end
    end
  end
end

class Event
  def initialize(data)
    @event_type = data['event_type']
    @code = data['code']
    @value = data['value']
    @timestamp_sec = data['timestamp_sec']
    @timestamp_nsec = data['timestamp_nsec']
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

  def to_s
    "Event(type=#{@event_type}, code=#{@code}, value=#{@value}, time=#{@timestamp_sec}.#{@timestamp_nsec})"
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
      query = { type: 'KeyState', key_code: key_code }
      send_to_makita("STATE:#{query.to_json}")

      false # TODO: implement
    end

    private

    def send_synthetic_event(event_type, code, value)
      event = {
        event_type: event_type,
        code: code,
        value: value
      }
      # TODO: implement
    end
  end
end

runtime = MakitaRuntime.new
Thread.current[:makita_runtime] = runtime
runtime.run
