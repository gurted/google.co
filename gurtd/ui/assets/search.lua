local function render(items)
    local list = gurt.select('#results')
    if list == nil then return end
    list.innerHTML = ''
    if type(items) ~= 'table' then return end
    for i=1,#items do
        local item = items[i] or {}
        local url = tostring(item.url or '')
        local title = tostring(item.title or '')
        local li = gurt.create('li', {
            style = 'w-full rounded border border-[#202637] bg-[#0f1526] hover:bg-[#111a2e] p-3 flex flex-col',
        }) 
        li:append(gurt.create('a', {
            href = url,
            style = 'text-[#e6e6f0] hover:text-[#6366f1] font-bold',
            text = (title ~= '' and title or url)
        }))
        li:append(gurt.create('div', {
            style = 'text-sm text-[#9ca3af] mt-1',
            text = url
        }))
        list:append(li)
    end
end
local function do_search(q)
  if q == nil or q == '' then return end

  local resp = fetch('/api/search?q=' .. encodeURIComponent(q))
  if resp and resp.status == 200 then
    local data = resp:json()
    if data and type(data.results) == 'table' then
      render(data.results)
    else
      render({})
    end
  end
end

local function get_query()
  local s = gurt.location.query.get('q') or ''
  if s == '' then return nil end
  return decodeURIComponent(s)
end

local function main()
    local input = gurt.select('#value')
    local submit = gurt.select('#submit')

    if input == nil or submit == nil then
        return false
    end

    local q = get_query()
    if q ~= nil and q ~= '' then
        if input then input.value = q end

        local list = gurt.select('#results')
        if not (list and list.children and #list.children > 0) then
            do_search(q)
        end
    end

    submit:on('click', function(ev)
        local query = (input and input.value) or ''

        if query == '' then return end
        
        do_search(query)
    end)
end

main()