function encodeURIComponent(str)
  if str == nil then return "" end
  return (tostring(str):gsub("[^%w%-_%.!~%*'%(%)]", function(c)
    local t = {}
    for i = 1, #c do
      t[#t+1] = string.format("%%%02X", c:byte(i))
    end
    return table.concat(t)
  end))
end

function decodeURIComponent(str)
  if str == nil then return "" end
  return (tostring(str):gsub("%%(%x%x)", function(h)
    return string.char(tonumber(h, 16))
  end))
end

function getPathname(href)
  local s = tostring(href or ""):match("^%s*(.-)%s*$")  -- trim
  local rest = s

  local after = rest:match("^%w[%w%+%-%.]*:%/*(.*)$")
  if after then rest = after end

  if rest:sub(1, 2) == "//" then rest = rest:sub(3) end

  local authority, tail = rest:match("^([^/%?#]*)(.*)$")
  authority = authority or ""
  tail = tail or ""

  local path = tail:match("^([^%?#]*)") or ""

  if authority ~= "" then
    if path == "" then return "/" end
    return (path == "" and "/" or path)
  else
    local abs = rest:match("^(/[^%?#]*)")
    if abs then return abs end
    return (rest:match("^([^%?#]*)") or "")
  end
end