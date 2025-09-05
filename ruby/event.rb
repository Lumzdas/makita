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
