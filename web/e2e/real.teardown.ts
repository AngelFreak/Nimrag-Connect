import { test as teardown } from '@playwright/test';
import { stopServer } from './real-server';

teardown('stop the real server and delete its data', async () => {
  await stopServer();
});
