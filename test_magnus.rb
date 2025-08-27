# Simple test script for Magnus integration
puts "Test script loaded successfully!"

# Test event handling
if defined?(event) && event
  puts "Event received: #{event}"
  puts "Event type: #{event.event_type}, code: #{event.code}, value: #{event.value}"

  # Test Makita module functionality
  if event.key_down? && event.code == 30  # KEY_A
    puts "A key pressed, testing synthetic events"
    Makita.press(48)  # Press B key
    puts "Synthetic B key press sent"
  end
else
  puts "No event context available"
end
