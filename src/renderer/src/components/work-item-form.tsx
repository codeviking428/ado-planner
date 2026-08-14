import { useMemo } from 'react'
import { useForm } from '@tanstack/react-form'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { DescriptionEditor } from '@/components/description-editor'
import { IdentityCombobox } from '@/components/identity-combobox'
import { showErrorToast } from '@/lib/error-toast'
import {
  allEditableControls,
  draftFromFormValues,
  formFieldName,
  workItemFormDefaults,
  workItemFormSchema
} from '@shared/form-layout'
import type { FormControl, IdentityValue, WorkItemFormModel } from '@shared/types'

function asInputValue(value: unknown): string {
  if (value === null || value === undefined) {
    return ''
  }
  if (typeof value === 'object' && value && 'displayName' in value) {
    return String((value as IdentityValue).displayName)
  }
  if (typeof value === 'string' && value.includes('T')) {
    return value.slice(0, 10)
  }
  return String(value)
}

function FieldControl({
  control,
  value,
  org,
  onChange
}: {
  control: FormControl
  value: unknown
  org: string
  onChange: (value: unknown) => void
}) {
  const id = control.referenceName
  if (control.kind === 'html') {
    return (
      <DescriptionEditor
        value={asInputValue(value)}
        onChange={onChange}
        readOnly={control.readOnly}
      />
    )
  }
  if (control.kind === 'identity') {
    const identity =
      value && typeof value === 'object'
        ? (value as IdentityValue)
        : value
          ? { displayName: String(value), uniqueName: String(value) }
          : null
    return (
      <IdentityCombobox
        org={org}
        value={identity}
        disabled={control.readOnly}
        onChange={onChange}
      />
    )
  }
  if (control.kind === 'boolean') {
    return (
      <input
        id={id}
        type="checkbox"
        checked={Boolean(value)}
        disabled={control.readOnly}
        onChange={(event) => onChange(event.target.checked)}
      />
    )
  }
  if (control.kind === 'picklist' && control.options) {
    return (
      <Select value={asInputValue(value)} disabled={control.readOnly} onValueChange={onChange}>
        <SelectTrigger id={id}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {control.options.map((option) => (
            <SelectItem key={option} value={option}>
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    )
  }
  const type =
    control.kind === 'dateTime'
      ? 'date'
      : control.kind === 'integer' || control.kind === 'double'
        ? 'number'
        : 'text'
  return (
    <Input
      id={id}
      type={type}
      disabled={control.readOnly}
      value={asInputValue(value)}
      onChange={(event) =>
        onChange(type === 'number' ? Number(event.target.value) : event.target.value)
      }
    />
  )
}

export function WorkItemFormDialog({
  org,
  project,
  model,
  onClose,
  onSaved
}: {
  org: string
  project: string
  model: WorkItemFormModel
  onClose: () => void
  onSaved: (rev: number, values: Record<string, unknown>) => void
}) {
  const editable = useMemo(() => allEditableControls(model), [model])
  const schema = useMemo(() => workItemFormSchema(editable), [editable])
  const defaultValues = useMemo(
    () => workItemFormDefaults(model.values, editable),
    [editable, model.values]
  )
  const pages =
    model.pages.length > 0 ? model.pages : [{ id: 'details', label: 'Details', groups: [] }]

  const form = useForm({
    defaultValues,
    validators: { onChange: schema },
    onSubmit: async ({ value }) => {
      const draft = draftFromFormValues(value)
      try {
        const result = await window.planner.ado.saveForm({
          org,
          project,
          id: model.id,
          rev: model.rev,
          original: model.values,
          draft,
          editable
        })
        toast.success(`Saved Work Item #${model.id}`)
        onSaved(result.rev, draft)
      } catch (error) {
        showErrorToast(error, 'Save failed')
      }
    }
  })

  const renderControl = (control: FormControl) => (
    <form.Field key={control.id} name={formFieldName(control.referenceName)}>
      {(field) => (
        <div className="grid gap-1.5">
          <Label htmlFor={control.referenceName}>{control.label}</Label>
          <FieldControl
            control={control}
            org={org}
            value={field.state.value}
            onChange={(value) => field.handleChange(value)}
          />
        </div>
      )}
    </form.Field>
  )

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        className="flex max-h-[90vh] w-full max-w-3xl flex-col sm:max-w-3xl"
        showCloseButton
      >
        <DialogHeader>
          <DialogTitle>
            #{model.id} {model.type}
          </DialogTitle>
          <DialogDescription>
            Layout pages as tabs. Groups are fieldsets on each page.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex min-h-0 flex-1 flex-col"
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            void form.handleSubmit()
          }}
        >
          <Tabs defaultValue={pages[0]?.id} className="min-h-0 flex-1">
            <TabsList variant="line" className="w-full justify-start">
              {pages.map((page) => (
                <TabsTrigger key={page.id} value={page.id}>
                  {page.label}
                </TabsTrigger>
              ))}
            </TabsList>
            {pages.map((page) => (
              <TabsContent key={page.id} value={page.id} className="min-h-0 overflow-auto pt-3">
                <ScrollArea className="max-h-[50vh] pr-2">
                  {page.id === pages[0]?.id
                    ? model.systemControls.map((control) => (
                        <div key={control.id} className="mb-3">
                          {renderControl(control)}
                        </div>
                      ))
                    : null}
                  {page.groups.map((group) => (
                    <fieldset key={group.id} className="mb-4 grid gap-3">
                      {group.label ? (
                        <legend className="text-sm font-medium">{group.label}</legend>
                      ) : null}
                      {group.controls.map((control) => renderControl(control))}
                    </fieldset>
                  ))}
                </ScrollArea>
              </TabsContent>
            ))}
          </Tabs>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <form.Subscribe selector={(state) => state.isSubmitting}>
              {(saving) => (
                <Button type="submit" disabled={saving} data-testid="save-work-item">
                  {saving ? 'Saving…' : 'Save'}
                </Button>
              )}
            </form.Subscribe>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
