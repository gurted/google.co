if type(network) ~= 'table' then
  network = {}
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
    trace.warn('No fetch API available; domain submission disabled')
  end
  return false
end

local statusBaseStyle = 'min-height: 24px'

local function update_status(kind, message)
  local status = gurt.select('#status')
  if status == nil then return end
  local style = statusBaseStyle
  if kind == 'error' then
    style = style .. '; color:#f87171'
  elseif kind == 'success' then
    style = style .. '; color:#4ade80'
  elseif kind == 'info' then
    style = style .. '; color:#e6e6f0'
  else
    style = style .. '; color:#9ca3af'
  end
  status:setAttribute('style', style)
  status.textContent = message
end

local function normalize_domain(raw)
  if raw == nil then return '' end
  local trimmed = raw:gsub('^%s+', ''):gsub('%s+$', '')
  if trimmed == '' then return '' end
  if trimmed:lower():sub(1, 7) == 'gurt://' then
    trimmed = trimmed:sub(8)
  end
  trimmed = trimmed:gsub('/+$', '')
  trimmed = trimmed:lower()
  return trimmed
end

local function validate_domain(domain)
  if domain == '' then return false end
  if #domain > 255 then return false end
  if domain:match('[^%w%.-]') then return false end
  return true
end

local function submit_domain(domain)
  if not ensure_network_fetch() then
    update_status('error', 'Network API unavailable')
    return
  end
  update_status('info', 'Submitting domain…')
  local payload = JSON.stringify({ domain = domain })
  local resp = network.fetch('/api/sites', {
    method = 'POST',
    headers = { ['content-type'] = 'application/json' },
    body = payload
  })
  if not resp then
    update_status('error', 'No response from server')
    return
  end
  if resp.status >= 200 and resp.status < 300 then
    local ok, data = pcall(function() return resp:json() end)
    if ok and data and data.domain then
      update_status('success', 'Accepted ' .. data.domain .. ' for indexing')
    else
      update_status('success', 'Domain accepted for indexing')
    end
  else
    local text = ''
    local ok, err = pcall(function() return resp:text() end)
    if ok and err then text = err end
    if text == '' then text = 'Request failed with status ' .. tostring(resp.status) end
    update_status('error', text)
  end
end

local function wire_form()
  local form = gurt.select('#domain-form')
  local input = gurt.select('#domain')
  local submit = gurt.select('#submit')
  if form == nil or input == nil then return false end
  if form:getAttribute('data-wired') == '1' then return true end
  form:setAttribute('data-wired', '1')

  local function handle(ev)
    if ev and ev.preventDefault then ev:preventDefault() end
    local value = ''
    if input and input.value then value = input.value end
    local domain = normalize_domain(value)
    if not validate_domain(domain) then
      update_status('error', 'Enter a valid domain (letters, digits, dots, and dashes)')
      return
    end
    submit_domain(domain)
    if input then input.value = '' end
  end

  form:on('submit', handle)
  if submit then
    submit:on('click', handle)
  end
  return true
end

if not wire_form() then
  if document and document.addEventListener then
    document.addEventListener('DOMContentLoaded', function() wire_form() end)
  end
  if window and window.addEventListener then
    window.addEventListener('load', function() wire_form() end)
  end
end

update_status('note', 'Ready to accept submissions')
