#!/usr/bin/env node
'use strict'

const { existsSync } = require('node:fs')
const { resolve } = require('node:path')
const { spawnSync } = require('node:child_process')

function run(command, args) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    shell: process.platform === 'win32'
  })
  if (result.error) {
    console.error(result.error)
    process.exit(1)
  }
  if (result.status) {
    process.exit(result.status)
  }
}

function isConsumerInstall() {
  if (process.env.npm_config_global === 'true') {
    return true
  }
  const init = process.env.INIT_CWD
  if (!init) {
    return false
  }
  return resolve(init) !== resolve(process.cwd())
}

const ci = process.env.CI === 'true'
const consumer = isConsumerInstall()

if (existsSync('.git') && !ci && !consumer) {
  spawnSync('lefthook', ['install'], {
    stdio: 'inherit',
    shell: process.platform === 'win32'
  })
}

if (consumer) {
  run('npx', ['--no-install', 'electron-vite', 'build'])
}
