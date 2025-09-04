module MakitaEventCodes
  EVENT_CODES_PATH = '/usr/include/linux/input-event-codes.h'

  module_function def couldnt_determine!(value)
    makita_log("warn", "Could not determine value from #{EVENT_CODES_PATH}: #{value}. Please contact the developer.")
  end

  module_function def determine(value)
    case value
    in /^\d+$/
      value.to_i
    in /^0x[0-9a-fA-F]+$/
      value.to_i(16)
    in /^[A-Z0-9_]+$/
      Makita.const_get(value)
    in String
      base, offset = value.match(/^\(([A-Z0-9_]+)\s{0,1}\+\s{0,1}(\d)\)$/)&.captures
      if base && offset
        Makita.const_get(base) + offset.to_i
      else
        couldnt_determine!(value)
      end
    else
      couldnt_determine!(value)
    end
  end

  module_function def define!
    File
      .read(EVENT_CODES_PATH)
      .lines
      .filter_map { _1.match(/^#define\s+(\S+)\s+(\(.*?\)|\S+)/)&.captures }
      .map { |name, value| Makita.const_set(name, determine(value)) }
  end
end

MakitaEventCodes.define!
