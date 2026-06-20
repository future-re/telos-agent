pub(super) const PAGE_SUMMARY_SCRIPT: &str = r#"
(() => JSON.stringify({
  url: location.href,
  title: document.title,
  ready_state: document.readyState,
  text_preview: (document.body?.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 4000)
}))()
"#;

pub(super) const BROWSER_STATE_SCRIPT: &str = r#"
(() => {
  const candidates = Array.from(document.querySelectorAll(
    'a,button,input,textarea,select,[role="button"],[role="link"],[onclick],[contenteditable="true"]'
  ));
  const visible = (el) => {
    const rect = el.getBoundingClientRect();
    const style = window.getComputedStyle(el);
    return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
  };
  let index = 0;
  const elements = [];
  for (const el of candidates) {
    if (elements.length >= 120) break;
    if (!visible(el)) continue;
    index += 1;
    const id = `e${index}`;
    el.setAttribute('data-telos-id', id);
    const rect = el.getBoundingClientRect();
    const tag = el.tagName.toLowerCase();
    const inputType = tag === 'input' ? (el.getAttribute('type') || 'text').toLowerCase() : null;
    const safeValue = inputType && ['password', 'hidden'].includes(inputType) ? '' : (el.value || '');
    elements.push({
      element_id: id,
      tag,
      type: inputType,
      text: (el.innerText || el.getAttribute('aria-label') || el.getAttribute('title') || el.value || '').replace(/\s+/g, ' ').trim().slice(0, 300),
      placeholder: el.getAttribute('placeholder') || '',
      name: el.getAttribute('name') || '',
      href: el.href || '',
      value: safeValue.slice(0, 300),
      disabled: !!el.disabled || el.getAttribute('aria-disabled') === 'true',
      rect: { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) }
    });
  }
  return JSON.stringify({
    url: location.href,
    title: document.title,
    ready_state: document.readyState,
    scroll: { x: window.scrollX, y: window.scrollY, max_y: document.documentElement.scrollHeight - window.innerHeight },
    viewport: { width: window.innerWidth, height: window.innerHeight },
    text_preview: (document.body?.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 8000),
    elements
  });
})()
"#;

pub(super) const BROWSER_CLICK_SCRIPT: &str = r#"
async (args) => {
  const el = findTelosElement(args);
  if (!el) return JSON.stringify({ ok: false, error: 'element not found' });
  el.scrollIntoView({ block: 'center', inline: 'center' });
  await new Promise(resolve => setTimeout(resolve, 60));
  el.click();
  return JSON.stringify({ ok: true, action: 'click', element: describeTelosElement(el) });
}
"#;

pub(super) const BROWSER_TYPE_SCRIPT: &str = r#"
async (args) => {
  const el = findTelosElement(args);
  if (!el) return JSON.stringify({ ok: false, error: 'element not found' });
  const text = args.text ?? '';
  el.scrollIntoView({ block: 'center', inline: 'center' });
  el.focus();
  const clear = args.clear !== false;
  if (el.isContentEditable) {
    if (clear) el.textContent = '';
    el.textContent = (el.textContent || '') + text;
  } else {
    if (clear) el.value = '';
    el.value = (el.value || '') + text;
  }
  el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return JSON.stringify({ ok: true, action: 'type', element: describeTelosElement(el), length: text.length });
}
"#;

pub(super) const BROWSER_SELECT_SCRIPT: &str = r#"
async (args) => {
  const el = findTelosElement(args);
  if (!el) return JSON.stringify({ ok: false, error: 'element not found' });
  if (el.tagName.toLowerCase() !== 'select') return JSON.stringify({ ok: false, error: 'element is not a select' });
  el.scrollIntoView({ block: 'center', inline: 'center' });
  el.value = args.value ?? '';
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return JSON.stringify({ ok: true, action: 'select', element: describeTelosElement(el), value: el.value });
}
"#;

pub(super) const BROWSER_ACTION_HELPERS: &str = r#"
function findTelosElement(args) {
  if (args.element_id) {
    const escaped = CSS.escape(args.element_id);
    const byId = document.querySelector(`[data-telos-id="${escaped}"]`);
    if (byId) return byId;
  }
  if (args.selector) {
    const bySelector = document.querySelector(args.selector);
    if (bySelector) return bySelector;
  }
  if (args.text) {
    const target = String(args.text).toLowerCase();
    const all = Array.from(document.querySelectorAll('a,button,input,textarea,select,[role="button"],[role="link"],[onclick],[contenteditable="true"]'));
    return all.find(el => ((el.innerText || el.value || el.getAttribute('aria-label') || el.getAttribute('title') || '').toLowerCase()).includes(target));
  }
  return null;
}
function describeTelosElement(el) {
  return {
    element_id: el.getAttribute('data-telos-id') || '',
    tag: el.tagName.toLowerCase(),
    text: (el.innerText || el.value || el.getAttribute('aria-label') || '').replace(/\s+/g, ' ').trim().slice(0, 200)
  };
}
"#;
