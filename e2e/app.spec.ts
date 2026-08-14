import { expect, test } from './fixtures'

test('seeded Session shows signed-in chrome and scope picker', async ({ window }) => {
  await expect(window.getByTestId('signed-in-chrome')).toBeVisible()
  await expect(window.getByTestId('session-name')).toHaveText('Ada Lovelace')
  await expect(window.locator('#org')).toHaveValue('contoso')
  await expect(window.locator('#project')).toHaveValue('Shop')
  await expect(window.locator('#team')).toHaveValue('Platform')
})

test('scope controls lock and show a spinner while switching organizations', async ({
  adoMock,
  window
}) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  adoMock.delayNext('/fabrikam/_apis/projects', 1_000)

  await window.locator('#org').selectOption('fabrikam')

  await expect(window.getByTestId('project-loading')).toBeVisible()
  await expect(window.locator('#org')).toBeDisabled()
  await expect(window.locator('#project')).toBeDisabled()
  await expect(window.locator('#team')).toBeDisabled()
  await expect(window.locator('#iteration')).toBeDisabled()

  await expect(window.locator('#project')).toHaveValue('Roadmap')
  await expect(window.locator('#team')).toHaveValue('Delivery')
  await expect(window.locator('#org')).toBeEnabled()
  await expect(window.locator('#project')).toBeEnabled()
  await expect(window.locator('#team')).toBeEnabled()
  await expect(window.locator('#iteration')).toBeEnabled()
})

test('scope errors show a toast whose message can be copied', async ({ adoMock, window }) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  await window.evaluate(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async (message: string) => {
          ;(
            window as typeof window & {
              copiedErrorMessage?: string
            }
          ).copiedErrorMessage = message
        }
      }
    })
  })
  adoMock.failNext('/fabrikam/_apis/projects', 503, 'Projects unavailable')

  await window.locator('#org').selectOption('fabrikam')

  const errorToast = window.locator('.cn-toast').filter({ hasText: 'Projects unavailable' }).first()
  await expect(errorToast).toBeVisible()
  const message = await errorToast.locator('[data-title]').innerText()
  await errorToast.getByRole('button', { name: 'Copy' }).click()
  await expect
    .poll(() =>
      window.evaluate(
        () =>
          (
            window as typeof window & {
              copiedErrorMessage?: string
            }
          ).copiedErrorMessage
      )
    )
    .toBe(message)
})

test('loads Hierarchy including an unparented root', async ({ window }) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  await expect(window.getByTestId('work-item-1003')).toContainText('Persist cart')
  await expect(window.getByTestId('work-item-1012')).toContainText('Unparented spike')
  await expect(window.getByTestId('dates-1012')).toHaveText(
    'Unscheduled · iteration 2026-08-10–2026-08-21'
  )
  await expect(window.getByTestId('dates-1003')).toHaveText('2026-07-06 → 2026-07-20')
})

test('drag date PATCHes Start/Target and shows a success toast', async ({ window }) => {
  const bar = window.locator('[data-slot="gantt-bar"]').first()
  await expect(bar).toBeVisible()
  const box = await bar.boundingBox()
  if (!box) {
    throw new Error('missing bar')
  }
  await window.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await window.mouse.down()
  await window.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2, { steps: 8 })
  await window.mouse.up()
  await expect(window.locator('.cn-toast').first()).toContainText(
    'Saved Start Date and Target Date'
  )
})

test('open and save Work Item form with a success toast', async ({ window }) => {
  await window.getByTestId('work-item-1003').dblclick()
  await expect(window.getByText('#1003 User Story')).toBeVisible()
  const title = window.locator('#System\\.Title')
  await title.fill('Persist cart (saved)')
  await window.getByTestId('save-work-item').click()
  await expect(window.locator('.cn-toast').first()).toContainText('Saved Work Item #1003')
})
