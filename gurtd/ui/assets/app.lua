local function get_origin(href)
  local trimmed = tostring(href or ''):match('^%s*(.-)%s*$')
  if trimmed == nil or trimmed == '' then return '' end
  local origin = trimmed:match('^(%w[%w%+%-%.]*:%/*[^/%?#]+)')
  return origin or ''
end

local function main()
  local input = gurt.select('#value')
  local submit = gurt.select('#submit')

  if input == nil or submit == nil then
    return false
  end

  submit:on('click', function(ev)
    local query = (input and input.value) or ''

    if query == '' then
      return
    end

    local origin = get_origin(gurt.location.href)
    local target = '/search?q=' .. encodeURIComponent(query or '')

    if origin ~= '' then
      target = origin .. target
    end
    gurt.location.goto(target)
  end)
end

main()