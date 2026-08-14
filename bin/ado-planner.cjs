#!/usr/bin/env node
'use strict'

const { spawn } = require('node:child_process')
const { existsSync } = require('node:fs')
const { join } = require('node:path')

const electron = require('electron')
const main = join(__dirname, '..', 'out', 'main', 'index.js')

if (!existsSync(main)) {
  console.error(
    'ADO Planner is not built yet. If you cloned the repo, run `pnpm build` then `ado-planner`.\n' +
      'If you installed with npm, re-run: npm install -g github:codeviking428/ado-planner'
  )
  process.exit(1)
}

const child = spawn(electron, [main, ...process.argv.slice(2)], {
  stdio: 'inherit',
  windowsHide: false
})

child.on('error', (error) => {
  console.error(error)
  process.exit(1)
})

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 0)
})
