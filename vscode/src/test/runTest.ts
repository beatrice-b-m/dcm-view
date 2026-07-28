import * as path from 'path';
import { runTests } from '@vscode/test-electron';

async function main(): Promise<void> {
  const extensionDevelopmentPath = path.resolve(__dirname, '..', '..');
  const extensionTestsPath = path.resolve(__dirname, 'suite', 'index');
  const version = process.env.DCMVIEW_VSCODE_TEST_VERSION?.trim() || '1.90.2';

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    version,
  });
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
