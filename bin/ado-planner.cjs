#!/usr/bin/env node
'use strict'

const { spawn } = require('node:child_process')
const { existsSync } = require('node:fs')
const { join } = require('node:path')

const main = join(__dirname, '..', 'out', 'main', 'index.js')

if (!existsSync(main)) {
  console.error(
    'ADO Planner is not built yet. If you cloned the repo, run `pnpm build` then `ado-planner`.\n' +
      'If you installed with npm, re-run: npm install -g github:codeviking428/ado-planner'
  )
  process.exit(1)
}

function localElectron() {
  try {
    return require('electron')
  } catch {
    return null
  }
}

function run(command, args) {
  const child = spawn(command, args, {
    stdio: 'inherit',
    windowsHide: false,
    shell: process.platform === 'win32' && command === 'npx'
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
}

const electron = localElectron()
if (electron) {
  run(electron, [main, ...process.argv.slice(2)])
} else {
  const spec = require('../package.json').devDependencies.electron
  run('npx', ['--yes', `electron@${spec}`, main, ...process.argv.slice(2)])
}
