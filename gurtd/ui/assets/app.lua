local function render(items)
  local list = gurt.select('#results')
  if list == nil then return end
  list.innerHTML = ''
  for i=1,#items do
    local li = document.createElement('li')
    local a = document.createElement('a')
    a.setAttribute('href', items[i].url)
    a.setAttribute('style', 'text-[#e6e6f0] hover:text-[#6366f1]')
    a.textContent = items[i].title ~= '' and items[i].title or items[i].url
    li.appendChild(a)
    list.appendChild(li)
  end
end

local function do_search(q)
  if q == nil or q == '' then return end
  -- call JSON API at /api/search
  local resp = fetch('/api/search?q=' .. encodeURIComponent(q))
  if resp and resp.status == 200 then
    local data = resp:json()
    render(data.results)
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
    gurt.location.goto('gurt://127.0.0.1:4878/search?q=' .. encodeURIComponent(q or ''))
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
    do_search(q)
  end
end
