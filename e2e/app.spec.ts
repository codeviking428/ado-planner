import type { Page } from '@playwright/test'
import { expect, test } from './fixtures'

async function chooseScope(window: Page, id: string, option: string) {
  await window.locator(`#${id}`).click()
  await window.getByRole('option', { name: option, exact: true }).click()
}

test('seeded Session shows signed-in chrome and scope picker', async ({ window }) => {
  await expect(window.getByTestId('signed-in-chrome')).toBeVisible()
  await expect(window.getByTestId('session-name')).toHaveText('Ada Lovelace')
  await expect(window.locator('#org')).toContainText('contoso')
  await expect(window.locator('#project')).toContainText('Shop')
  await expect(window.locator('#team')).toContainText('Platform')
  await expect(window.locator('#iteration')).toContainText('All')
  await expect(window.locator('#assignee')).toContainText('Anyone')
  await expect(window.locator('#iteration')).not.toContainText('__none__')
})

test('scope controls lock and show a spinner while switching organizations', async ({
  adoMock,
  window
}) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  adoMock.delayNext('/fabrikam/_apis/projects', 1_000)

  await chooseScope(window, 'org', 'fabrikam')

  await expect(window.getByTestId('project-loading')).toBeVisible()
  await expect(window.locator('#org')).toBeDisabled()
  await expect(window.locator('#project')).toBeDisabled()
  await expect(window.locator('#team')).toBeDisabled()
  await expect(window.locator('#iteration')).toBeDisabled()

  await expect(window.locator('#project')).toContainText('Roadmap')
  await expect(window.locator('#team')).toContainText('Delivery')
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

  await chooseScope(window, 'org', 'fabrikam')

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
  await expect(window.getByTestId('work-item-1012')).toBeHidden()
  await expect(window.getByTestId('dates-1003')).toHaveText('2026-07-06 → 2026-07-20')
})

test('Roots hide unparented Work Items that are not a Root type', async ({ window }) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  await expect(window.getByTestId('work-item-1012')).toBeHidden()
  await expect(window.getByTestId('root-filter')).toContainText('3 hidden')
  await window.getByTestId('root-filter').click()
  await window.getByRole('menuitem', { name: 'Show all' }).click()
  await expect(window.getByTestId('work-item-1012')).toContainText('Unparented spike')
  await expect(window.getByTestId('dates-1012')).toContainText('Unscheduled')
  await window.getByTestId('root-filter').click()
  await window.getByRole('menuitem', { name: 'Hide all' }).click()
  await expect(window.getByTestId('empty-gantt')).toBeVisible()
})

test('assignee menu lists Team members and filters the Hierarchy', async ({ window }) => {
  await expect(window.getByTestId('work-item-1003')).toBeVisible()
  await window.locator('#assignee').click()
  await expect(window.getByRole('option', { name: 'Ada Lovelace' })).toBeVisible()
  await expect(window.getByRole('option', { name: 'Grace Hopper' })).toBeVisible()
  await window.getByRole('option', { name: 'Grace Hopper' }).click()
  await expect(window.getByTestId('work-item-1003')).toBeHidden()
  await chooseScope(window, 'assignee', 'Anyone')
  await expect(window.getByTestId('work-item-1003')).toBeVisible()
})

test('scope Team search filters options', async ({ window }) => {
  await window.locator('#team').click()
  await window.getByTestId('team-search').fill('Plat')
  await expect(window.getByRole('option', { name: 'Platform' })).toBeVisible()
  await expect(window.getByRole('option', { name: 'Select Team' })).toBeHidden()
})

test('scope and filters persist across reload', async ({ window }) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  await chooseScope(window, 'org', 'fabrikam')
  await expect(window.locator('#project')).toContainText('Roadmap')
  await expect(window.locator('#team')).toContainText('Delivery')
  await window.getByTestId('type-filter').click()
  await window.getByRole('menuitemcheckbox', { name: 'Task' }).click()
  await expect(window.getByTestId('type-filter')).toContainText('1 hidden')
  await window.reload()
  await expect(window.getByTestId('signed-in-chrome')).toBeVisible()
  await expect(window.locator('#org')).toContainText('fabrikam')
  await expect(window.locator('#project')).toContainText('Roadmap')
  await expect(window.locator('#team')).toContainText('Delivery')
  await expect(window.getByTestId('type-filter')).toContainText('1 hidden')
})

test('type filter search narrows the list', async ({ window }) => {
  await window.getByTestId('type-filter').click()
  await window.getByTestId('type-filter-search').fill('Task')
  await expect(window.getByRole('menuitemcheckbox', { name: 'Task' })).toBeVisible()
  await expect(window.getByRole('menuitemcheckbox', { name: 'Epic' })).toBeHidden()
})

test('type filter hides a leaf Work Item and can show it again', async ({ window }) => {
  await expect(window.getByTestId('work-item-1005')).toBeVisible()
  await window.getByTestId('type-filter').click()
  await window.getByRole('menuitemcheckbox', { name: 'Task' }).click()
  await expect(window.getByTestId('work-item-1005')).toBeHidden()
  await expect(window.getByTestId('type-filter')).toContainText('1 hidden')
  await window.getByRole('menuitemcheckbox', { name: 'Task' }).click()
  await expect(window.getByTestId('work-item-1005')).toBeVisible()
})

test('type filter Hide all clears the Hierarchy', async ({ window }) => {
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
  await window.getByTestId('type-filter').click()
  await window.getByRole('menuitem', { name: 'Hide all' }).click()
  await expect(window.getByTestId('empty-gantt')).toBeVisible()
  await window.getByTestId('type-filter').click()
  await window.getByRole('menuitem', { name: 'Show all' }).click()
  await expect(window.getByTestId('work-item-1001')).toBeVisible()
})

test('Work Item titles and types are not clipped', async ({ window }) => {
  await expect(window.getByTestId('work-item-1003')).toBeVisible()
  const overflowing = await window.getByTestId('work-item-1003').evaluate((el) => {
    const title = el.querySelector('span.truncate') ?? el
    return title.scrollWidth > title.clientWidth + 1
  })
  expect(overflowing).toBe(false)
  await expect(window.getByText('User Story', { exact: true }).first()).toBeVisible()
  await window.screenshot({
    path: 'test-results/planner-board.png',
    fullPage: true
  })
  await window.getByTestId('type-filter').click()
  await expect(window.getByRole('menuitemcheckbox', { name: 'Epic' })).toBeVisible()
  await window.screenshot({
    path: 'test-results/planner-filters.png'
  })
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
