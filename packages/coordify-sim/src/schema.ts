import Ajv from 'ajv';

export interface ScenarioStep {
  delay_ms: number;
  event: Record<string, unknown>;
}

export interface ScenarioScript {
  name: string;
  agents: string[];
  steps: ScenarioStep[];
  finalize?: boolean;
}

const SCHEMA = {
  type: 'object',
  required: ['name', 'agents', 'steps'],
  additionalProperties: true,
  properties: {
    name: { type: 'string', minLength: 1 },
    agents: { type: 'array', items: { type: 'string' } },
    finalize: { type: 'boolean' },
    steps: {
      type: 'array',
      items: {
        type: 'object',
        required: ['delay_ms', 'event'],
        properties: {
          delay_ms: { type: 'number', minimum: 0 },
          event: { type: 'object', required: ['type'], properties: { type: { type: 'string' } } },
        },
      },
    },
  },
};

const ajv = new Ajv();
const validate = ajv.compile(SCHEMA);

export function validateScript(raw: unknown): ScenarioScript | string[] {
  const valid = validate(raw);
  if (!valid) return (validate.errors ?? []).map(e => `${e.instancePath || '/'} ${e.message}`);
  return raw as ScenarioScript;
}
