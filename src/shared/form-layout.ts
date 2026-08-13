import type {
  FormControl,
  FormControlKind,
  FormGroup,
  FormPage,
  JsonPatchOp,
  WorkItemFormModel
} from './types'

const SKIP_CONTROL_TYPES = new Set([
  'WorkItemLogControl',
  'LinksControl',
  'AttachmentsControl',
  'History',
  'Links',
  'Attachments'
])

const SKIP_SYSTEM_IDS = new Set(['System.History', 'System.Links', 'System.AttachedFiles'])

export type ProcessLayout = {
  systemControls?: LayoutControl[]
  pages?: LayoutPage[]
}

export type LayoutPage = {
  id?: string
  label?: string
  visible?: boolean
  pageType?: string
  sections?: Array<{
    groups?: LayoutGroup[]
  }>
}

export type LayoutGroup = {
  id?: string
  label?: string
  visible?: boolean
  isContribution?: boolean
  controls?: LayoutControl[]
}

export type LayoutControl = {
  id?: string
  label?: string
  visible?: boolean
  readOnly?: boolean
  controlType?: string
  isContribution?: boolean
}

export type FieldMetadata = {
  referenceName: string
  name?: string
  type?: string
  isIdentity?: boolean
  isPicklist?: boolean
  alwaysRequired?: boolean
  readOnly?: boolean
  allowedValues?: string[]
}

export function stripMnemonic(label: string): string {
  return label.replaceAll('&', '')
}

export function kindFromMetadata(
  control: LayoutControl,
  meta: FieldMetadata | undefined
): FormControlKind {
  if (meta?.isIdentity) {
    return 'identity'
  }
  if (control.controlType === 'HtmlFieldControl' || meta?.type === 'html') {
    return 'html'
  }
  if (control.controlType === 'WorkItemClassificationControl' || meta?.type === 'treePath') {
    return 'treePath'
  }
  if (meta?.isPicklist || (meta?.allowedValues && meta.allowedValues.length > 0)) {
    return 'picklist'
  }
  switch (meta?.type) {
    case 'integer':
      return 'integer'
    case 'double':
      return 'double'
    case 'boolean':
      return 'boolean'
    case 'dateTime':
      return 'dateTime'
    case 'plainText':
      return 'plainText'
    case 'html':
      return 'html'
    default:
      return 'string'
  }
}

function toFormControl(
  control: LayoutControl,
  metaByRef: Map<string, FieldMetadata>
): FormControl | null {
  const id = control.id
  if (!id || SKIP_SYSTEM_IDS.has(id) || SKIP_CONTROL_TYPES.has(control.controlType ?? '')) {
    return null
  }
  if (control.isContribution || control.visible === false) {
    return null
  }
  const meta = metaByRef.get(id)
  return {
    id,
    referenceName: id,
    label: stripMnemonic(control.label ?? meta?.name ?? id),
    kind: kindFromMetadata(control, meta),
    required: meta?.alwaysRequired === true,
    readOnly: control.readOnly === true || meta?.readOnly === true,
    visible: true,
    options: meta?.allowedValues
  }
}

export function flattenLayout(
  layout: ProcessLayout,
  fields: FieldMetadata[]
): { systemControls: FormControl[]; pages: FormPage[] } {
  const metaByRef = new Map(fields.map((field) => [field.referenceName, field]))
  const systemControls = (layout.systemControls ?? [])
    .map((control) => toFormControl(control, metaByRef))
    .filter((control): control is FormControl => control !== null)

  const pages: FormPage[] = []
  for (const page of layout.pages ?? []) {
    if (page.visible === false || page.pageType !== 'custom') {
      continue
    }
    const groups: FormGroup[] = []
    for (const section of page.sections ?? []) {
      for (const group of section.groups ?? []) {
        if (group.visible === false || group.isContribution) {
          continue
        }
        const controls = (group.controls ?? [])
          .map((control) => toFormControl(control, metaByRef))
          .filter((control): control is FormControl => control !== null)
        if (controls.length === 0) {
          continue
        }
        groups.push({
          id: group.id ?? group.label ?? 'group',
          label: stripMnemonic(group.label ?? ''),
          controls
        })
      }
    }
    pages.push({
      id: page.id ?? page.label ?? 'page',
      label: stripMnemonic(page.label ?? 'Details'),
      groups
    })
  }

  return { systemControls, pages }
}

export function allEditableControls(model: WorkItemFormModel): FormControl[] {
  return [
    ...model.systemControls,
    ...model.pages.flatMap((page) => page.groups.flatMap((group) => group.controls))
  ].filter((control) => control.visible && !control.readOnly)
}

export function buildFormPatch(input: {
  rev: number
  original: Record<string, unknown>
  draft: Record<string, unknown>
  editable: FormControl[]
}): JsonPatchOp[] {
  const ops: JsonPatchOp[] = [{ op: 'test', path: '/rev', value: input.rev }]
  for (const control of input.editable) {
    const next = input.draft[control.referenceName]
    const prev = input.original[control.referenceName]
    if (Object.is(next, prev) || JSON.stringify(next) === JSON.stringify(prev)) {
      continue
    }
    ops.push({
      op: 'add',
      path: `/fields/${control.referenceName}`,
      value: next ?? ''
    })
  }
  return ops
}
