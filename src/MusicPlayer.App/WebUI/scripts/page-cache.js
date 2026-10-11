export function createPageCache(limit = 6) {
  const pages = new Map();
  return {
    get(key) { const value = pages.get(key); if (value !== undefined) { pages.delete(key); pages.set(key, value); } return value; },
    set(key, value) { pages.delete(key); pages.set(key, value); while (pages.size > limit) pages.delete(pages.keys().next().value); },
    clear() { pages.clear(); },
    get size() { return pages.size; }
  };
}
