// The opencode.json of the C7 film's config card, and of the fixture home the capture runs a
// real backend with (capture/real.ts), so the film shows the file ref read.
export const OPENCODE_JSON = [
  '{',
  '  "model": "anthropic/claude-sonnet-4-5",',
  '  "provider": {',
  '    "local": {',
  '      "npm": "@ai-sdk/openai-compatible",',
  '      "options": {',
  '        "baseURL": "http://localhost:8080/v1",',
  '        "apiKey": "{env:WORK_API_KEY}"',
  '      },',
  '      "models": { "qwen3-coder": {} }',
  '    }',
  '  },',
  '  "agent": { "review": { "description": "Reviews the diff" } },',
  '  "permission": { "bash": "ask" }',
  '}',
]
