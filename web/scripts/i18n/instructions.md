MoleSignal is an open-source, self-hostable observability and incident-response product: logs, metrics, traces, RUM, alerting, on-call, status pages and an AI agent.

- The key path tells where a string appears (a button, a column header, a toast). Use it as context; never translate it.
- Keep every {{placeholder}} exactly as written: same name, same double braces. Reorder them only when the grammar needs it. Keep literal text inside single braces such as {field} unchanged.
- Keep <0>…</0> style tags and HTML tags exactly, translating only the text between them.
- Do not translate "MoleSignal", "Mole Agent", other product and vendor names, code identifiers, query languages (SQL, PromQL), units, or technical tokens (URLs, MIME types, header names, field names in backticks).
- Use the terminology established in localized observability products (Datadog, Grafana, New Relic) for the target language, and translate the same term the same way every time.
- Keep translations concise and natural. Use sentence case where the language has case. Keep leading and trailing whitespace and the ellipsis character (…).

Follow the guidance for the target language:

- zh-tw: The source is a reviewed Simplified Chinese translation. Keep its meaning and wording choices and write Traditional Chinese as used in Taiwan, with Taiwan software vocabulary rather than a character-by-character conversion: 伺服器, 資料, 程式, 檔案, 設定, 介面, 預設, 網路, 訊息, 使用者, 登入, 帳號.
- ja-jp: Use the polite です/ます style for sentences and plain noun phrases for labels and buttons, as Japanese SaaS consoles do. Write established technical terms in katakana (ダッシュボード, アラート, トレース).
- ko-kr: Use the formal 합니다 style for sentences and short noun phrases for labels and buttons, as Korean SaaS consoles do.
- de: Address the reader with "Sie". Keep labels short; prefer established German IT terms but keep the English ones Germans use (Dashboard, Trace, Span, Log).
- es: Neutral international Spanish. Address the reader with "tú", as current Spanish software does; avoid words particular to Spain or to any single Latin American country.
- fr: Address the reader with "vous". Use French typography: a non-breaking space before : ; ? ! and inside « ».
- pt-br: Brazilian Portuguese. Address the reader with "você". Use Brazilian vocabulary (arquivo, tela, usuário), not European Portuguese.
