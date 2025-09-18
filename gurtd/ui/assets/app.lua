local function render(items)
  local list = gurt.select('#results')
  if list == nil then return end
  list.innerHTML = ''
  if type(items) ~= 'table' then return end
  for i=1,#items do
    local item = items[i] or {}
    local url = tostring(item.url or '')
    local title = tostring(item.title or '')
    local li = document and document.createElement and document.createElement('li') or nil
    if li == nil then
      -- Fallback render if DOM createElement is unavailable
  list.innerHTML = list.innerHTML .. '<li style="w-full rounded border border-[#202637] bg-[#0f1526] hover:bg-[#111a2e] p-3">'
        .. '<a href="' .. url .. '" style="text-[#e6e6f0] hover:text-[#6366f1] font-bold">' .. (title ~= '' and title or url) .. '</a>'
        .. '<div style="text-sm text-[#9ca3af] mt-1">' .. url .. '</div>'
        .. '</li>'
    else
  li.setAttribute('style', 'w-full rounded border border-[#202637] bg-[#0f1526] hover:bg-[#111a2e] p-3')
      local a = document.createElement('a')
      a.setAttribute('href', url)
      a.setAttribute('style', 'text-[#e6e6f0] hover:text-[#6366f1] font-bold')
      a.textContent = (title ~= '' and title or url)
      li.appendChild(a)
      local small = document.createElement('div')
      small.setAttribute('style', 'text-sm text-[#9ca3af] mt-1')
      small.textContent = url
      li.appendChild(small)
      list.appendChild(li)
    end
  end
end

if type(network) ~= 'table' then
  network = {}
end

local function get_origin(href)
  local trimmed = tostring(href or ''):match('^%s*(.-)%s*$')
  if trimmed == nil or trimmed == '' then return '' end
  local origin = trimmed:match('^(%w[%w%+%-%.]*:%/*[^/%?#]+)')
  return origin or ''
end

local function ensure_network_fetch()
  if type(network.fetch) == 'function' then
    return true
  end
  if type(fetch) == 'function' then
    network.fetch = fetch
    return true
  end
  if trace and trace.warn then
    trace.warn('No fetch API available; search requests disabled')
  end
  return false
end

local function do_search(q)
  if q == nil or q == '' then return end
  -- call JSON API at /api/search
  if not ensure_network_fetch() then return end
  local resp = network.fetch('/api/search?q=' .. encodeURIComponent(q))
  if resp and resp.status == 200 then
    local data = resp:json()
    if data and type(data.results) == 'table' then
      render(data.results)
    else
      render({})
    end
  end
end

-- wire form submit with DOM-ready fallback (handles cases where script runs before DOM exists)
local function wire_form()
  local valueInput = gurt.select('#value')
  local submitButton = gurt.select('#submit')
  local form = gurt.select('#form')
  if valueInput == nil then return false end
  if submitButton == nil then return false end
  if form == nil then return false end

  print("wire_form")

  if form:getAttribute('data-wired') == '1' then return true end
  form:setAttribute('data-wired', '1')

  submitButton:on('click', function(ev)
    print("submit")
    local q = ''
    if valueInput and valueInput.value then q = valueInput.value end
    local origin = get_origin(gurt.location.href)
    local target = '/search?q=' .. encodeURIComponent(q or '')
    if origin ~= '' then
      target = origin .. target
    end
    gurt.location.goto(target)
  end)
  return true
end

-- attempt immediate; if DOM not ready, bind after load
if not wire_form() then
  if document and document.addEventListener then
    document.addEventListener('DOMContentLoaded', function() wire_form() end)
  end
  if window and window.addEventListener then
    window.addEventListener('load', function() wire_form() end)
  end
end

-- restore state on load
local function get_query()
  local s = gurt.location.query.get('q') or ''
  if s == '' then return nil end
  return decodeURIComponent(s)
end

if getPathname(gurt.location.href) == '/search' then
  local q = get_query()
  if q ~= nil and q ~= '' then
    local qel = gurt.select('#q')
    if qel then qel.value = q end
    -- If SSR already rendered results (ul has children), skip refetch
    local list = gurt.select('#results')
    local has_children = (list and list.children and #list.children > 0)
    if not has_children then
      do_search(q)
    end
  end
  -- Wire the search page form to submit via XHR
  local form = gurt.select('#qform')
  if form then
    if form:getAttribute('data-wired') ~= '1' then
      form:setAttribute('data-wired', '1')
      form:on('submit', function(ev)
        if ev and ev.preventDefault then ev:preventDefault() end
        local input = gurt.select('#q')
        local value = (input and input.value) or ''
        do_search(value)
      end)
    end
  end
end
