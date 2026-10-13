import { StreamLanguage } from '@codemirror/language'
import { properties } from '@codemirror/legacy-modes/mode/properties'
import { yaml } from '@codemirror/legacy-modes/mode/yaml'
import { EditorView } from '@codemirror/view'
import { githubDark, githubLight } from '@uiw/codemirror-theme-github'
import CodeMirror from '@uiw/react-codemirror'
import { useTheme } from 'next-themes'
import { useMemo, useState } from 'react'
import { Button } from '@/components/ui/button'

const PROPERTIES = StreamLanguage.define(properties)
const YAML = StreamLanguage.define(yaml)

function language(format: string) {
  return format === 'yaml' ? YAML : PROPERTIES
}

export function NativeConfigCodeEditor({ content, format, disabled, saving, onSave }: {
  content: string
  format: string
  disabled?: boolean
  saving?: boolean
  onSave: (content: string) => void
}) {
  const { resolvedTheme } = useTheme()
  const [draft, setDraft] = useState(content)
  const extensions = useMemo(() => [language(format), EditorView.lineWrapping], [format])
  const dirty = draft !== content
  return <div className="flex flex-col gap-3"><div className="overflow-hidden border"><CodeMirror value={draft} height="60vh" theme={resolvedTheme === 'dark' ? githubDark : githubLight} extensions={extensions} editable={!disabled} onChange={setDraft} /></div><div className="flex items-center gap-3"><Button type="button" disabled={disabled || !dirty || saving} onClick={() => onSave(draft)}>Save code</Button>{dirty && <p className="text-xs text-muted-foreground">Unsaved changes are discarded when switching files.</p>}</div></div>
}
