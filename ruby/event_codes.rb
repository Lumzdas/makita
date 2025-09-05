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

    Makita.const_set(:CHAR_TO_KEYCODE, {
      ' ' => [Makita.const_get(:KEY_SPACE), :lower],
      '!' => [Makita.const_get(:KEY_1), :upper],
      '@' => [Makita.const_get(:KEY_2), :upper],
      '#' => [Makita.const_get(:KEY_3), :upper],
      '$' => [Makita.const_get(:KEY_4), :upper],
      '%' => [Makita.const_get(:KEY_5), :upper],
      '^' => [Makita.const_get(:KEY_6), :upper],
      '&' => [Makita.const_get(:KEY_7), :upper],
      '*' => [Makita.const_get(:KEY_8), :upper],
      '(' => [Makita.const_get(:KEY_9), :upper],
      ')' => [Makita.const_get(:KEY_0), :upper],
      '-' => [Makita.const_get(:KEY_MINUS), :lower],
      '_' => [Makita.const_get(:KEY_MINUS), :upper],
      '=' => [Makita.const_get(:KEY_EQUAL), :lower],
      '+' => [Makita.const_get(:KEY_EQUAL), :upper],
      '[' => [Makita.const_get(:KEY_LEFTBRACE), :lower],
      '{' => [Makita.const_get(:KEY_LEFTBRACE), :upper],
      ']' => [Makita.const_get(:KEY_RIGHTBRACE), :lower],
      '}' => [Makita.const_get(:KEY_RIGHTBRACE), :upper],
      '\\' => [Makita.const_get(:KEY_BACKSLASH), :lower],
      '|' => [Makita.const_get(:KEY_BACKSLASH), :upper],
      ';' => [Makita.const_get(:KEY_SEMICOLON), :lower],
      ':' => [Makita.const_get(:KEY_SEMICOLON), :upper],
      "'" => [Makita.const_get(:KEY_APOSTROPHE), :lower],
      '"' => [Makita.const_get(:KEY_APOSTROPHE), :upper],
      ',' => [Makita.const_get(:KEY_COMMA), :lower],
      '<' => [Makita.const_get(:KEY_COMMA), :upper],
      '.' => [Makita.const_get(:KEY_DOT), :lower],
      '>' => [Makita.const_get(:KEY_DOT), :upper],
      '/' => [Makita.const_get(:KEY_SLASH), :lower],
      '?' => [Makita.const_get(:KEY_SLASH), :upper],
      "\n" => [Makita.const_get(:KEY_ENTER), :lower],
      "\t" => [Makita.const_get(:KEY_TAB), :lower],
    })
  end
end

MakitaEventCodes.define!
