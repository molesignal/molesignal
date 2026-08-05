/** Build a small but valid rrweb recording for demo/seed sessions. */
export function buildRrwebReplay({
  startMs,
  origin,
  journey,
  interaction = 'click',
  errorLabel,
  viewport = { width: 1440, height: 900 },
}) {
  let nextId = 1;
  const refs = {};
  const remember = (key, id) => {
    if (key) refs[key] = id;
    return id;
  };
  const text = (textContent, key) => ({
    type: 3,
    textContent,
    id: remember(key, nextId++),
  });
  const element = (tagName, attributes = {}, childNodes = [], key) => ({
    type: 2,
    tagName,
    attributes,
    childNodes,
    id: remember(key, nextId++),
  });

  const firstPage = journey[0] ?? '/';
  const finalPage = journey[journey.length - 1] ?? firstPage;
  const pageTitle = titleForPage(firstPage);
  const style = `
    *{box-sizing:border-box}body{margin:0;background:#f6f7fb;color:#172033;font-family:Inter,system-ui,sans-serif}
    header{height:64px;display:flex;align-items:center;justify-content:space-between;padding:0 32px;background:#fff;border-bottom:1px solid #e2e6ef}
    .brand{font-weight:750;color:#4747d7}.shell{display:grid;grid-template-columns:220px 1fr;min-height:836px}
    nav{padding:28px 20px;background:#fff;border-right:1px solid #e2e6ef}.nav-item{padding:10px 12px;margin-bottom:6px;border-radius:8px;color:#69738a}.active{background:#eeeeff;color:#4747d7}
    main{padding:52px 7vw}.eyebrow{color:#6d73e8;font-size:13px;font-weight:700;text-transform:uppercase;letter-spacing:.08em}
    h1{margin:10px 0 12px;font-size:38px}.lede{max-width:700px;color:#69738a}.cards{display:grid;grid-template-columns:repeat(3,1fr);gap:18px;margin-top:34px}
    .card{min-height:150px;padding:22px;border:1px solid #e2e6ef;border-radius:14px;background:#fff}.card strong{display:block;margin-bottom:10px}.price{font-size:24px;font-weight:750}
    .checkout{display:flex;align-items:center;justify-content:space-between;margin-top:26px;padding:20px 22px;border-radius:14px;background:#fff;border:1px solid #e2e6ef}
    button{border:0;border-radius:9px;background:#4747d7;color:#fff;padding:12px 20px;font-weight:700}@media(max-width:700px){.shell{grid-template-columns:1fr}nav{display:none}.cards{grid-template-columns:1fr}main{padding:28px 20px}}
  `;
  const documentNode = {
    type: 0,
    id: nextId++,
    childNodes: [
      { type: 1, name: 'html', publicId: '', systemId: '', id: nextId++ },
      element('html', { lang: 'en' }, [
        element('head', {}, [
          element('meta', { charset: 'utf-8' }),
          element('meta', {
            name: 'viewport',
            content: 'width=device-width, initial-scale=1',
          }),
          element('title', {}, [text(`Mole Shop · ${pageTitle}`)]),
          element('style', {}, [text(style)]),
        ]),
        element('body', {}, [
          element('header', {}, [
            element('span', { class: 'brand' }, [text('Mole Shop')]),
            element('span', {}, [text('Secure checkout')]),
          ]),
          element('div', { class: 'shell' }, [
            element('nav', {}, [
              element('div', { class: 'nav-item active' }, [text('Store')]),
              element('div', { class: 'nav-item' }, [text('Orders')]),
              element('div', { class: 'nav-item' }, [text('Account')]),
            ]),
            element('main', {}, [
              element('p', { class: 'eyebrow' }, [text('Recorded customer journey')]),
              element('h1', {}, [text(pageTitle, 'headingText')]),
              element('p', { class: 'lede' }, [
                text('This page is reconstructed from a real rrweb DOM snapshot and timestamped mutations.'),
              ]),
              element('div', { class: 'cards' }, [
                productCard(element, text, 'Running shoes', '$129'),
                productCard(element, text, 'Travel pack', '$89'),
                productCard(element, text, 'Studio headphones', '$199'),
              ]),
              element('div', { class: 'checkout' }, [
                element('span', {}, [
                  element('strong', {}, [text('Order ready')]),
                  element('span', { class: 'price' }, [text('$218')]),
                ]),
                element(
                  'button',
                  { id: 'checkout-primary-action', type: 'button' },
                  [text('Continue checkout')],
                  'actionButton',
                ),
              ]),
            ]),
          ]),
        ]),
      ]),
    ],
  };
  const href = (page) => new URL(page, origin).toString();
  const events = [
    {
      type: 4,
      data: { href: href(firstPage), ...viewport },
      timestamp: startMs,
    },
    {
      type: 2,
      data: { node: documentNode, initialOffset: { left: 0, top: 0 } },
      timestamp: startMs + 1,
    },
    ...journey.map((page, index) => ({
      type: 'view',
      ts: startMs + index * 12_000,
      href: href(page),
      name: `View ${page}`,
    })),
  ];

  for (const [index, page] of journey.slice(1).entries()) {
    events.push({
      type: 3,
      data: {
        source: 0,
        texts: [{ id: refs.headingText, value: titleForPage(page) }],
        attributes: [],
        removes: [],
        adds: [],
      },
      timestamp: startMs + (index + 1) * 12_000,
    });
  }
  events.push(
    {
      type: 3,
      data: { source: 3, id: documentNode.id, x: 0, y: 320 },
      timestamp: startMs + 36_000,
    },
    {
      type: 3,
      data: {
        source: 1,
        positions: [{ x: 1_120, y: 610, id: refs.actionButton, timeOffset: 0 }],
      },
      timestamp: startMs + 46_000,
    },
  );
  const clickCount = interaction === 'rage_click' ? 3 : 1;
  for (let index = 0; index < clickCount; index += 1) {
    events.push({
      type: 3,
      data: { source: 2, type: 2, id: refs.actionButton, x: 1_120, y: 610 },
      timestamp: startMs + 48_000 + index * 180,
    });
  }
  events.push({
    type: interaction,
    ts: startMs + 48_000,
    href: href(finalPage),
    selector: '#checkout-primary-action',
    name:
      interaction === 'rage_click'
        ? 'Repeated checkout submission'
        : interaction === 'dead_click'
          ? 'Unresponsive checkout control'
          : interaction === 'crash'
            ? 'Checkout page crashed'
            : 'Continue checkout',
  });
  if (errorLabel) {
    events.push({
      type: 'error',
      ts: startMs + 52_000,
      href: href(finalPage),
      name: errorLabel,
    });
  }
  return events.sort((left, right) => eventTime(left) - eventTime(right));
}

function productCard(element, text, name, price) {
  return element('article', { class: 'card' }, [
    element('strong', {}, [text(name)]),
    element('span', { class: 'price' }, [text(price)]),
  ]);
}

function titleForPage(page) {
  if (page === '/') return 'Welcome back';
  return page
    .split('/')
    .filter(Boolean)
    .map((part) => part.replace(/[-_]/g, ' '))
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' · ');
}

function eventTime(event) {
  return Number(event.timestamp ?? event.ts ?? 0);
}
