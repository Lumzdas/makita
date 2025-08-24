# propagate input
def handle(event)
  Makima.press(event.key) if event.key_down?
end
