// jsdom has no layout: give elements a size so virtualized lists render rows, and stub the observers.
if (typeof window !== 'undefined') {
  const rect = { x: 0, y: 0, top: 0, left: 0, bottom: 800, right: 1000, width: 1000, height: 800, toJSON: () => ({}) }
  Element.prototype.getBoundingClientRect = function () { return rect as DOMRect }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, get: () => 800 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, get: () => 1000 })
  Element.prototype.scrollTo = function () {}
  window.scrollTo = function () {} // the router restores scroll on navigation; jsdom logs "not implemented" otherwise
  Element.prototype.scrollIntoView = function () {}
  globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
}
