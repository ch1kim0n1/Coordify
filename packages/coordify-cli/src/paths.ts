import path from 'path';
import fs from 'fs';

export const coordify = (root: string) => path.join(root, '.coordify');
export const runtime = (root: string) => path.join(coordify(root), 'runtime');
export const socket = (root: string) => path.join(runtime(root), 'core.sock');
export const token = (root: string) => path.join(runtime(root), 'session.token');
export const sessions = (root: string) => path.join(coordify(root), 'sessions');
export const sessionDir = (root: string, id: string) => path.join(sessions(root), id);
export const knowledgeDir = (root: string) => path.join(coordify(root), 'knowledge');

export function readToken(root: string): string | null {
  try { return fs.readFileSync(token(root), 'utf8').trim(); } catch { return null; }
}
