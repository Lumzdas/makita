#!/usr/bin/env ruby

require 'json'
require 'fiber'

# Global state for the Ruby event loop
class MakitaRuntime
  def initialize
    @scripts = {}
    @fibers = {}
    @sleeping_fibers = []
    @synthetic_events = []
    @state_queries = []
  end

  # Load a script from file
  def load_script(name, path)
    begin
      content = File.read(path)
      @scripts[name] = content
      puts "LOADED:#{name}"
      $stdout.flush
    rescue => e
      puts "ERROR:Failed to load script #{name}: #{e.message}"
      $stdout.flush
    end
  end

  # Handle incoming physical event
  def handle_event(event_data)
    event = Event.new(event_data)

    # Process all scripts that might handle this event
    @scripts.each do |script_name, script_content|
      # Create or reuse fiber for this script
      fiber = @fibers[script_name] || create_script_fiber(script_name, script_content)
      @fibers[script_name] = fiber

      # Resume fiber with the event if it's not dead
      if fiber.alive?
        begin
          result = fiber.resume(event)
          # If script returns nil, it consumes the event
          if result.nil?
            puts "CONSUME:#{script_name}"
            $stdout.flush
          end
        rescue => e
          puts "ERROR:Script #{script_name} error: #{e.message}"
          $stdout.flush
          # Remove dead fiber
          @fibers.delete(script_name)
        end
      end
    end

    # Send any synthetic events that were generated
    flush_synthetic_events
  end

  # Create a new fiber for script execution
  def create_script_fiber(script_name, script_content)
    Fiber.new do |initial_event|
      # Set up the script environment
      eval(script_content)

      # Main script loop - handle events one by one
      current_event = initial_event
      loop do
        if current_event
          # Call the script's handle method
          result = handle(current_event) if respond_to?(:handle)
          # Yield control back to main loop
          current_event = Fiber.yield(result)
        else
          # No more events, yield and wait
          current_event = Fiber.yield
        end
      end
    end
  end

  # Process sleeping fibers
  def process_sleeping_fibers
    current_time = Time.now
    @sleeping_fibers.reject! do |sleep_info|
      if current_time >= sleep_info[:wake_time]
        # Wake up the fiber
        if sleep_info[:fiber].alive?
          begin
            sleep_info[:fiber].resume
          rescue => e
            puts "ERROR:Fiber resume error: #{e.message}"
            $stdout.flush
          end
        end
        true # Remove from sleeping list
      else
        false # Keep sleeping
      end
    end
  end

  # Flush synthetic events to stdout
  def flush_synthetic_events
    while !@synthetic_events.empty?
      event = @synthetic_events.shift
      puts "SYNTHETIC:#{event.to_json}"
      $stdout.flush
    end
  end

  # Main event loop
  def run
    puts "READY"
    $stdout.flush

    while true
      # Check for sleeping fibers
      process_sleeping_fibers

      # Read input with timeout to avoid blocking
      if IO.select([STDIN], nil, nil, 0.001) # 1ms timeout
        begin
          line = STDIN.gets
          break if line.nil? # EOF

          line = line.strip
          next if line.empty?

          if line.start_with?("LOAD:")
            parts = line[5..].split(":", 2)
            if parts.length == 2
              load_script(parts[0], parts[1])
            end
          elsif line.start_with?("EVENT:")
            event_json = line[6..]
            begin
              event_data = JSON.parse(event_json)
              handle_event(event_data)
            rescue JSON::ParserError => e
              puts "ERROR:Invalid JSON: #{e.message}"
              $stdout.flush
            end
          end
        rescue => e
          puts "ERROR:Input processing error: #{e.message}"
          $stdout.flush
        end
      end

      # Small sleep to prevent busy waiting
      sleep(0.001)
    end
  end
end

# Event class that Ruby scripts will use
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

# Makita module for Ruby scripts
module Makita
  # Key constants
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

    # Send synthetic key press event
    def press(key_code)
      send_synthetic_event(1, key_code, 1)  # EV_KEY, key_code, press
      send_synthetic_event(1, key_code, 0)  # EV_KEY, key_code, release
    end

    # Send synthetic key press down (hold)
    def press_down(*key_codes)
      key_codes.each do |key_code|
        send_synthetic_event(1, key_code, 1)  # EV_KEY, key_code, press
      end
    end

    # Send synthetic key release
    def release(key_code)
      send_synthetic_event(1, key_code, 0)  # EV_KEY, key_code, release
    end

    # Query key state from Rust (synchronous)
    def get_key_state(key_code)
      query = { type: 'KeyState', key_code: key_code }
      puts "STATE:#{query.to_json}"
      $stdout.flush
      # In a real implementation, we'd wait for a response
      # For now, return false as default
      false
    end

    # Fiber-aware sleep that doesn't block the event loop
    def sleep(seconds)
      current_fiber = Fiber.current
      wake_time = Time.now + seconds

      runtime.instance_variable_get(:@sleeping_fibers) << {
        fiber: current_fiber,
        wake_time: wake_time
      }

      # Yield control back to event loop
      Fiber.yield
    end

    private

    def send_synthetic_event(event_type, code, value)
      event = {
        event_type: event_type,
        code: code,
        value: value
      }
      runtime.instance_variable_get(:@synthetic_events) << event
    end
  end
end

# Set up the runtime and start the event loop
runtime = MakitaRuntime.new
Thread.current[:makita_runtime] = runtime
runtime.run
