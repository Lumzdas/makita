#!/usr/bin/env ruby

require 'json'
require 'fiber'

class Runtime
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
      pipe_read_fd = makita_get_signal_pipe_read_fd
      makita_log("debug", "Ruby app: Obtained pipe read FD: #{pipe_read_fd}")
      pipe_io = IO.for_fd(pipe_read_fd, autoclose: false)

      loop do
        pipe_io.wait_readable
        pipe_io.read_nonblock(1)

        makita_get_events.each do |event_data|
          script_name = event_data['script']
          if script = @scripts[script_name]
            event = Event.new(event_data)
            Fiber.schedule do
              eval(script)
            rescue => e
              makita_log("error", "Event processing error in #{script_name}: #{e.message}")
              makita_log("error", "    from #{e.backtrace.first}")
            end
          else
            makita_log("error", "Script not loaded: #{script_name}")
          end
        end
      end
    end

    Fiber.scheduler.run
  end
end

# Initialize global runtime instance
# This will be created from Rust side, so we don't run it automatically
