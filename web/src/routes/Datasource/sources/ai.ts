/** LLM providers and agent frameworks. */

import { ENDPOINT, TOKEN, type Source } from './types';

export const AI_SOURCES: Source[] = [
  {
    id: 'openai',
    category: 'ai',
    signals: ['logs', 'traces'],
    steps: [
      {
        code: {
          lang: 'python',
          content: `from traceloop.sdk import Traceloop

Traceloop.init(
  app_name="my-app",
  api_endpoint="${ENDPOINT}/api",
  headers={"authorization": "Bearer ${TOKEN}"},
)

# every openai.* call is instrumented from here on`,
        },
      },
    ],
  },
  {
    id: 'anthropic',
    category: 'ai',
    signals: ['logs', 'traces'],
    steps: [
      {
        code: {
          lang: 'python',
          content: `from traceloop.sdk import Traceloop
Traceloop.init(app_name="my-app", api_endpoint="${ENDPOINT}/api", headers={"authorization": "Bearer ${TOKEN}"})

# anthropic.Anthropic() / .messages.create() are traced automatically`,
        },
      },
    ],
  },
  {
    id: 'langchain',
    category: 'ai',
    signals: ['logs', 'traces'],
    steps: [
      {
        code: {
          lang: 'python',
          content: `from langchain_molesignal import MoleSignalCallback
chain = LLMChain(...).with_config({"callbacks": [MoleSignalCallback()]})`,
        },
      },
    ],
  },
  {
    id: 'llamaindex',
    category: 'ai',
    signals: ['traces'],
    steps: [{}],
  },
];
