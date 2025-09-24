---@meta
-- Gurted UI typings for Lua Language Server (EmmyLua annotations)

---@alias EventHandler fun(ev: Event|nil)

---@class Event
---@field x number|nil
---@field y number|nil
---@field deltaX number|nil
---@field deltaY number|nil
---@field key string|nil
---@field keycode integer|nil
---@field ctrl boolean|nil
---@field shift boolean|nil
---@field alt boolean|nil
---@field value any
---@field fileName string|nil
---@field data table<string, any>|nil
local Event = {}
---@return nil
function Event:preventDefault() end

---@class Element
---@field innerHTML string
---@field textContent string
---@field text string|nil
---@field value any
---@field visible boolean
---@field children Element[]|nil
---@field size { width: number, height: number }|nil
---@field position { x: number, y: number }|nil
---@field parent Element|nil
---@field nextSibling Element|nil
---@field previousSibling Element|nil
---@field firstChild Element|nil
---@field lastChild Element|nil
---@field classList ElementClassList|nil
local Element = {}
---@param name string
---@param value string
---@return nil
function Element:setAttribute(name, value) end
---@param name string
---@return string|nil
function Element:getAttribute(name) end
---@param child Element
---@return nil
function Element:appendChild(child) end
---@param event string
---@param handler EventHandler
---@return Subscription
function Element:on(event, handler) end
---@param child Element
---@return nil
function Element:append(child) end
---@return nil
function Element:remove() end
---@param newElement Element
---@param referenceElement Element
---@return nil
function Element:insertBefore(newElement, referenceElement) end
---@param newElement Element
---@param referenceElement Element
---@return nil
function Element:insertAfter(newElement, referenceElement) end
---@param oldElement Element
---@param newElement Element
---@return nil
function Element:replace(oldElement, newElement) end
---@param deep boolean
---@return Element
function Element:clone(deep) end
---@return nil
function Element:show() end
---@return nil
function Element:hide() end
---@return nil
function Element:focus() end
---@return nil
function Element:unfocus() end
---@param kind "'2d'"|"'shader'"
---@return Canvas2DContext|ShaderContext
function Element:withContext(kind) end
---@return Tween
function Element:createTween() end

---@class Document
---@field createElement fun(tag: string): Element
---@field addEventListener fun(event: string, handler: EventHandler)
local Document = {}

---@class Window
---@field addEventListener fun(event: string, handler: EventHandler)
local Window = {}

---@class Response
---@field status integer
---@field statusText string
---@field headers { [string]: string }
local Response = {}
---@return any
function Response:json() end
---@return string
function Response:text() end
---@return boolean
function Response:ok() end

---@class Network
---@field fetch fun(url: string, options?: table): Response
local Network = {}

---@class JSONLib
---@field stringify fun(value: any): string
---@field parse fun(json: string): any, string|nil
local JSONLib = {}

---@class Trace
---@field log fun(message: string)
---@field warn fun(message: string)
---@field error fun(message: string)
local Trace = {}

---@class GurtLocationQuery
local GurtLocationQuery = {}
---@param name string
---@return string|nil
function GurtLocationQuery:get(name) end
---@param name string
---@return boolean
function GurtLocationQuery:has(name) end
---@param name string
---@return string[]
function GurtLocationQuery:getAll(name) end

---@class GurtLocation
---@field href string
---@field query GurtLocationQuery
---@field ["goto"] fun(url: string)
---@field reload fun()
local GurtLocation = {}

---@alias UIElement Element|AudioElement|CanvasElement

---@class CreateOptions
---@field text string|nil
---@field style string|nil
---@field id string|nil
local CreateOptions = {}

---@class Gurt
---@field location GurtLocation
---@field body Element
---@field select fun(selector: string): UIElement|nil
---@field selectAll fun(selector: string): UIElement[]
---@field create fun(tagName: string, options?: CreateOptions): UIElement
---@field width fun(): number
---@field height fun(): number
---@field crumbs GurtCrumbs
local Gurt = {}

-- Subscription for event listeners
---@class Subscription
local Subscription = {}
---@return nil
function Subscription:unsubscribe() end

-- Element class list management
---@class ElementClassList
---@field length integer
local ElementClassList = {}
---@param class string
---@return nil
function ElementClassList:add(class) end
---@param class string
---@return nil
function ElementClassList:remove(class) end
---@param class string
---@return boolean
function ElementClassList:contains(class) end
---@param class string
---@return nil
function ElementClassList:toggle(class) end
---@param index integer
---@return string|nil
function ElementClassList:item(index) end

-- Specialized elements
---@class AudioElement: Element
---@field currentTime number
---@field duration number
---@field volume number
---@field loop boolean
---@field src string
---@field playing boolean
---@field paused boolean
local AudioElement = {}
---@return nil
function AudioElement:play() end
---@return nil
function AudioElement:pause() end
---@return nil
function AudioElement:stop() end

---@class CanvasElement: Element
---@field width integer
---@field height integer
local CanvasElement = {}

-- Canvas and drawing
---@class TextMetrics
---@field width number

---@class Canvas2DContext
local Canvas2DContext = {}
---@param x number @left
---@param y number @top
---@param width number
---@param height number
---@param color string|nil
---@return nil
function Canvas2DContext:fillRect(x, y, width, height, color) end
---@param x number
---@param y number
---@param width number
---@param height number
---@param color string|nil
---@param strokeWidth number|nil
---@return nil
function Canvas2DContext:strokeRect(x, y, width, height, color, strokeWidth) end
---@param x number
---@param y number
---@param width number
---@param height number
---@return nil
function Canvas2DContext:clearRect(x, y, width, height) end
---@param x number
---@param y number
---@param radius number
---@param color string|nil
---@param filled boolean|nil
---@return nil
function Canvas2DContext:drawCircle(x, y, radius, color, filled) end
---@param x number
---@param y number
---@param text string
---@param color string|nil
---@overload fun(x: number, y: number, text: string, font: string, color?: string)
---@return nil
function Canvas2DContext:drawText(x, y, text, color) end
---@param spec string
---@return nil
function Canvas2DContext:setFont(spec) end
---@param text string
---@return TextMetrics
function Canvas2DContext:measureText(text) end
---@return nil
function Canvas2DContext:beginPath() end
---@param x number
---@param y number
---@return nil
function Canvas2DContext:moveTo(x, y) end
---@param x number
---@param y number
---@return nil
function Canvas2DContext:lineTo(x, y) end
---@return nil
function Canvas2DContext:closePath() end
---@return nil
function Canvas2DContext:stroke() end
---@return nil
function Canvas2DContext:fill() end
---@param x number
---@param y number
---@param radius number
---@param startAngle number
---@param endAngle number
---@param counterclockwise boolean|nil
---@return nil
function Canvas2DContext:arc(x, y, radius, startAngle, endAngle, counterclockwise) end
---@param cpx number
---@param cpy number
---@param x number
---@param y number
---@return nil
function Canvas2DContext:quadraticCurveTo(cpx, cpy, x, y) end
---@param cp1x number
---@param cp1y number
---@param cp2x number
---@param cp2y number
---@param x number
---@param y number
---@return nil
function Canvas2DContext:bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y) end
---@param color string
---@return nil
function Canvas2DContext:setStrokeStyle(color) end
---@param color string
---@return nil
function Canvas2DContext:setFillStyle(color) end
---@param width number
---@return nil
function Canvas2DContext:setLineWidth(width) end
---@return nil
function Canvas2DContext:save() end
---@param x number
---@param y number
---@return nil
function Canvas2DContext:translate(x, y) end
---@param radians number
---@return nil
function Canvas2DContext:rotate(radians) end
---@param sx number
---@param sy number
---@return nil
function Canvas2DContext:scale(sx, sy) end
---@return nil
function Canvas2DContext:restore() end

---@class ShaderContext
local ShaderContext = {}
---@param code string
---@return nil
function ShaderContext:source(code) end

-- Tweening
---@class Tween
local Tween = {}
---@param property string
---@param value any
---@return Tween
function Tween:to(property, value) end
---@param seconds number
---@return Tween
function Tween:duration(seconds) end
---@param easing "'in'"|"'out'"|"'inout'"|"'outin'"
---@return Tween
function Tween:easing(easing) end
---@param transition "'linear'"|"'quad'"|"'cubic'"|"'quart'"|"'quint'"|"'sine'"|"'expo'"|"'circ'"|"'elastic'"|"'back'"|"'bounce'"
---@return Tween
function Tween:transition(transition) end
---@return nil
function Tween:play() end

-- Crumbs API
---@class Crumb
---@field name string
---@field value string
---@field expiry number|nil

---@class GurtCrumbs
local GurtCrumbs = {}
---@param options { name: string, value: string, lifetime?: number }
---@return nil
function GurtCrumbs.set(options) end
---@param name string
---@return string|nil
function GurtCrumbs.get(name) end
---@param name string
---@return boolean
function GurtCrumbs.delete(name) end
---@return table<string, Crumb>
function GurtCrumbs.getAll() end

-- WebSocket
---@class WebSocketMessage
---@field data string
---@class WebSocketError
---@field message string
---@class WebSocket
WebSocket = WebSocket or {}
---@param url string
---@return WebSocket
function WebSocket.new(url) end
---@param event string
---@param handler fun(arg?: any)
---@return Subscription
function WebSocket:on(event, handler) end
---@param data string
---@return nil
function WebSocket:send(data) end
---@return nil
function WebSocket:close() end

-- Regex
---@class Regex
Regex = Regex or {}
---@param pattern string
---@return Regex
function Regex.new(pattern) end
---@param text string
---@return boolean
function Regex:test(text) end
---@param text string
---@return string[]|nil
function Regex:match(text) end

-- Clipboard
---@class ClipboardLib
---@field write fun(text: string)
---@type ClipboardLib
Clipboard = Clipboard or {}
---@param text string
---@return nil
function Clipboard.write(text) end

-- Time utilities
---@class Timer
local Timer = {}
---@return number
function Timer:elapsed() end
---@return nil
function Timer:reset() end

---@class Delay
local Delay = {}
---@return boolean
function Delay:complete() end
---@return number
function Delay:remaining() end

---@class TimeLib
---@field now fun(): number
---@field format fun(timestamp: number, format: string): string
---@field date fun(timestamp: number): { year: integer, month: integer, day: integer, hour: integer, minute: integer, second: integer, weekday: integer }
---@field sleep fun(seconds: number)
---@field benchmark fun(fn: fun(): any): number, any
---@field timer fun(): Timer
---@field delay fun(seconds: number): Delay
---@type TimeLib
Time = Time or {}
---@return number
function Time.now() end
---@param timestamp number
---@param format string
---@return string
function Time.format(timestamp, format) end
---@param timestamp number
---@return { year: integer, month: integer, day: integer, hour: integer, minute: integer, second: integer, weekday: integer }
function Time.date(timestamp) end
---@param seconds number
---@return nil
function Time.sleep(seconds) end
---@param fn fun(): any
---@return number elapsed
---@return any result
function Time.benchmark(fn) end
---@return Timer
function Time.timer() end
---@param seconds number
---@return Delay
function Time.delay(seconds) end

-- Scheduling
---@param cb fun()
---@param milliseconds integer
---@return integer timeoutId
function setTimeout(cb, milliseconds) end
---@param id integer
function clearTimeout(id) end
---@param cb fun()
---@param milliseconds integer
---@return integer intervalId
function setInterval(cb, milliseconds) end
---@param id integer
function clearInterval(id) end
---@param cb fun()
function onNextFrame(cb) end

-- URL helpers
---@param s string
---@return string
function urlEncode(s) end
---@param s string
---@return string
function urlDecode(s) end

-- JSON.parse (two returns: data, error)
---@param json string
---@return any data
---@return string|nil error
function JSON.parse(json) end

-- String helpers
---@param text string
---@param search string|Regex
---@param replacement string
---@return string
function string.replace(text, search, replacement) end
---@param text string
---@param search string|Regex
---@param replacement string
---@return string
function string.replaceAll(text, search, replacement) end
---@param text string
---@return string
function string.trim(text) end

-- Table helper
---@param t table
---@return string
function table.tostring(t) end




-- Global bindings (types only; no runtime effect due to @meta)
---@type Gurt
gurt = {}
---@type Document
document = {}
---@type Window
window = {}
---@type Network
network = {}
---@type JSONLib
JSON = {}
---@type Trace
trace = {}
---@type fun(url: string, options?: table): Response
function fetch(url, options) end

---@param str string|nil
---@return string
function encodeURIComponent(str) end
---@param str string|nil
---@return string
function decodeURIComponent(str) end
---@param href string|nil
---@return string
function getPathname(href) end