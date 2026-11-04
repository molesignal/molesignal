# Field query contract

The field query is the text of the Logs and Traces pages: `level = 'error' AND (service.name = 'checkout' OR timeout)`. Three readers parse it and must agree:

- the web, which turns it into builder rows and request fields (`web/src/shell/query/fieldExpression.ts`, `web/src/routes/logs/fieldQueryModel.ts`, `web/src/routes/traces/fieldQueryModel.ts`);
- the server, which reads the `expression` parameter of the log and trace reads (`crates/engines/query-language/src/field_query/`);
- the CLI (`molesignal-cli/src/fieldQuery.ts`), which keeps its own copy.

`cases.json` is the shared test table. Each case is a query and either the request fields it reads into, exactly as the web sends them (`filters`, `freeText` and `where` for logs; `q`, `filters` and `where` for traces), or `"error": true` for a query no reader accepts. The web runs it in `web/src/routes/logs/fieldQueryContract.test.ts` and `web/src/routes/traces/fieldQueryContract.test.ts`, and the server in the `query-language` crate's tests. A change to the grammar changes this table and both readers in the same change.

Whether a field exists and takes an operator depends on the stream. The web checks it while reading a trace query; the server checks it when the read validates its filters. The trace cases therefore only use fields and operators the stream used by the web test accepts.
