# propagate input
def handle(event)
  Makita.press(event.key) if event.key_down?
end
