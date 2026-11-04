import type { Page } from '@playwright/test';

/**
 * Signs in with a password the way a person does, one question at a time. Starts from a page that
 * lists a way in, as the shared mock's does: it has a provider.
 */
export async function signInWithPassword(page: Page, email: string, password: string) {
  await page.getByRole('button', { name: 'Sign in with email' }).click();
  await page.getByLabel('Email').fill(email);
  await page.getByRole('button', { name: 'Continue →', exact: true }).click();
  await page.getByLabel('Password', { exact: true }).fill(password);
  await page.getByRole('button', { name: 'Sign in →', exact: true }).click();
}
