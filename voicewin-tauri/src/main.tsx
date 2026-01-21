import React from 'react';
import ReactDOM from 'react-dom/client';
import { isTauri } from '@tauri-apps/api/core';
import { App } from './ui/App';
import { createMockInvoker, createTauriInvoker } from './lib/invoker';
import './ui/styles.css';

async function bootstrap() {
  const invoker = isTauri() ? await createTauriInvoker() : createMockInvoker();

  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App invoker={invoker} />
    </React.StrictMode>,
  );
}

void bootstrap();
