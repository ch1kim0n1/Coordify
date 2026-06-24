import React, { useState, useEffect } from 'react';
import { render, useApp, useInput } from 'ink';
import ReplayFrame from './replay-watch.js';

function ReplayApp({ events, speed: initSpeed }: { events: Record<string, unknown>[]; speed: number }) {
  const { exit } = useApp();
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const [speed, setSpeed] = useState(initSpeed);

  useInput((input, key) => {
    if (input === 'q' || (key.ctrl && input === 'c')) { exit(); return; }
    if (input === ' ') { setPaused(p => !p); return; }
    if (key.rightArrow) setIndex(i => Math.min(i + 10, events.length - 1));
    if (key.leftArrow)  setIndex(i => Math.max(i - 10, 0));
    if (input === '+') setSpeed(s => Math.min(s * 2, 4));
    if (input === '-') setSpeed(s => Math.max(s / 2, 0.5));
  });

  useEffect(() => {
    if (paused || events.length === 0) return;
    if (index >= events.length - 1) { exit(); return; }
    const delay = Math.max(50, 1000 / speed);
    const t = setTimeout(() => setIndex(i => i + 1), delay);
    return () => clearTimeout(t);
  }, [index, paused, speed, events.length]);

  return React.createElement(ReplayFrame, { events, currentIndex: index, total: events.length, speed, paused });
}

export async function runReplayApp(events: Record<string, unknown>[], speed: number): Promise<void> {
  const { waitUntilExit } = render(React.createElement(ReplayApp, { events, speed }));
  await waitUntilExit();
}
