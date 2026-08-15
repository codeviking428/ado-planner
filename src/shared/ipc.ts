import { z } from 'zod'

export const loginCredsSchema = z
  .object({
    pat: z.string().optional(),
    organization: z.string().optional()
  })
  .optional()

export const scopeSchema = z.object({
  org: z.string().min(1),
  project: z.string().min(1),
  team: z.string().min(1),
  iterationPath: z.string().nullable().optional()
})

export const overlaySchema = z.object({
  types: z.array(z.string()).nullable(),
  states: z.array(z.string()).nullable(),
  assignee: z.string().min(1),
  rootTypes: z.array(z.string()).nullable().optional(),
  iterationPath: z.string().nullable(),
  currentUserUniqueName: z.string().nullable().optional()
})

export const patchDatesSchema = z.object({
  org: z.string().min(1),
  project: z.string().min(1),
  id: z.number().int().positive(),
  rev: z.number().int().nonnegative(),
  startDate: z.string().min(1),
  targetDate: z.string().min(1)
})

export const openFormSchema = z.object({
  org: z.string().min(1),
  project: z.string().min(1),
  id: z.number().int().positive()
})

export const saveFormSchema = z.object({
  org: z.string().min(1),
  project: z.string().min(1),
  id: z.number().int().positive(),
  rev: z.number().int().nonnegative(),
  original: z.record(z.string(), z.unknown()),
  draft: z.record(z.string(), z.unknown()),
  editable: z.array(
    z.object({
      id: z.string(),
      referenceName: z.string(),
      label: z.string(),
      kind: z.string(),
      required: z.boolean(),
      readOnly: z.boolean(),
      visible: z.boolean(),
      options: z.array(z.string()).optional()
    })
  )
})

export const searchIdentitiesSchema = z.object({
  org: z.string().min(1),
  query: z.string().min(1)
})

export const teamMembersSchema = z.object({
  org: z.string().min(1),
  project: z.string().min(1),
  team: z.string().min(1)
})
