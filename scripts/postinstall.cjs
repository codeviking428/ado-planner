#!/usr/bin/env node
'use strict'

try {
  require.resolve('electron-builder/package.json')
} catch {
  process.exit(0)
}

const { spawnSync } = require('node:child_process')
const result = spawnSync('electron-builder', ['install-app-deps'], {
  stdio: 'inherit',
  shell: process.platform === 'win32'
})
process.exit(result.status ?? 0)
