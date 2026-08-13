import { useEffect } from 'react'
import { useEditor, EditorContent } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Placeholder from '@tiptap/extension-placeholder'

import { cn } from '@/lib/utils'

type DescriptionEditorProps = {
  value: string
  onChange: (html: string) => void
  readOnly?: boolean
}

export function DescriptionEditor({ value, onChange, readOnly }: DescriptionEditorProps) {
  const editor = useEditor({
    extensions: [StarterKit, Placeholder.configure({ placeholder: 'HTML Description' })],
    content: value || '<p></p>',
    editable: !readOnly,
    immediatelyRender: false,
    onUpdate: ({ editor: instance }) => {
      onChange(instance.getHTML())
    }
  })

  useEffect(() => {
    if (!editor) {
      return
    }
    const current = editor.getHTML()
    if (value !== current) {
      editor.commands.setContent(value || '<p></p>', { emitUpdate: false })
    }
  }, [editor, value])

  useEffect(() => {
    editor?.setEditable(!readOnly)
  }, [editor, readOnly])

  return (
    <div
      className={cn(
        'border-input bg-background min-h-32 rounded-lg border px-3 py-2 text-sm',
        readOnly && 'opacity-70'
      )}
    >
      <EditorContent
        editor={editor}
        className="prose prose-sm dark:prose-invert max-w-none [&_.tiptap]:outline-none"
      />
    </div>
  )
}
