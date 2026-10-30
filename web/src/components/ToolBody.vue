<script setup lang="ts">
// The expanded part of a tool card, laid out by tool: a diff for edits, the command and its exit
// status for bash, numbered lines for read and grep, paths for find, and so on. Anything else
// shows its input and output as they are.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolBlock } from '../model/blocks.ts'
import {
  metaFlag,
  metaNumber,
  metaString,
  parseFound,
  parseSourceListing,
  peerName,
  stringArg,
} from '../model/toolContent.ts'
import CommandBox from './CommandBox.vue'
import DiffView from './DiffView.vue'
import FoundList from './FoundList.vue'
import Icon from './Icon.vue'
import ImageThumbs from './ImageThumbs.vue'
import SourceLines from './SourceLines.vue'
import ToolChip from './ToolChip.vue'
import ToolOutput from './ToolOutput.vue'
import ToolSection from './ToolSection.vue'
import ToolText from './ToolText.vue'

const props = defineProps<{ block: ToolBlock }>()

const JSON_INDENT = 2
const PEER_ICONS = {
  send_message: 'chat-send-16',
  create_session: 'chat-plus-16',
  read_session: 'chat-16',
  list_sessions: 'chat-list-16',
  message_status: 'chat-send-16',
  close_session: 'chat-x-16',
} as const

const { t } = useI18n()

const name = computed(() => props.block.name)
const meta = computed(() => props.block.meta)
const output = computed(() => props.block.output ?? '')
const diff = computed(() => metaString(meta.value, 'diff'))
const showsDiff = computed(() => ['edit', 'write'].includes(name.value) && !!diff.value?.trim())
const command = computed(() => stringArg(props.block.input, 'command'))
const listing = computed(() =>
  ['read', 'grep'].includes(name.value) ? parseSourceListing(output.value) : undefined,
)
const found = computed(() => (name.value === 'find' && output.value ? parseFound(output.value) : undefined))
const peerIcon = computed(() => PEER_ICONS[name.value as keyof typeof PEER_ICONS])
const peer = computed(() => peerName(name.value, props.block.input))
const message = computed(
  () => stringArg(props.block.input, 'message') ?? stringArg(props.block.input, 'prompt'),
)
const url = computed(() => metaString(meta.value, 'url') ?? stringArg(props.block.input, 'url'))

// Which layout applies: the first tool-specific one whose data is there, else the generic one.
const layout = computed(() => {
  if (showsDiff.value) return 'diff'
  if (name.value === 'bash' && command.value !== undefined) return 'bash'
  if (listing.value) return name.value
  if (found.value) return 'find'
  if (name.value === 'webfetch' && url.value) return 'webfetch'
  if (peerIcon.value && name.value !== 'list_sessions') return 'peer'
  return 'generic'
})

const inputText = computed(() => {
  const { input, rawInput } = props.block
  const empty = input === null || (typeof input === 'object' && Object.keys(input).length === 0)
  return empty ? rawInput : JSON.stringify(input, null, JSON_INDENT)
})
const exitCode = computed(() => metaNumber(meta.value, 'exit_code'))
const bytes = computed(() => metaNumber(meta.value, 'bytes'))
const status = computed(() => metaNumber(meta.value, 'status'))
const truncated = computed(() => metaFlag(meta.value, 'truncated'))
const countOf = (key: string) => metaNumber(meta.value, key) ?? 0

function open(target: string): void {
  window.open(target, '_blank', 'noopener,noreferrer')
}
</script>

<template>
  <div class="body">
    <template v-if="layout === 'diff'">
      <ToolSection>
        <div class="path">
          <span class="file">{{ metaString(meta, 'path') }}</span>
          <span class="added">+{{ countOf('added') }}</span>
          <span class="removed">-{{ countOf('removed') }}</span>
        </div>
        <DiffView :diff="diff!" />
      </ToolSection>
      <ToolOutput v-if="block.state === 'error'" :block="block" />
    </template>

    <template v-else-if="layout === 'bash'">
      <ToolSection><CommandBox :command="command!" /></ToolSection>
      <ToolOutput :block="block">
        <template #chips>
          <ToolChip v-if="exitCode !== undefined" :danger="exitCode !== 0">exit {{ exitCode }}</ToolChip>
          <ToolChip v-if="metaFlag(meta, 'timed_out')" danger>{{ t('tool.timedOut') }}</ToolChip>
          <ToolChip v-if="metaFlag(meta, 'cancelled')">{{ t('tool.cancelled') }}</ToolChip>
          <ToolChip v-if="truncated">{{ t('tool.truncated') }}</ToolChip>
        </template>
      </ToolOutput>
    </template>

    <template v-else-if="layout === 'read' || layout === 'grep'">
      <ToolSection v-for="file in listing!.files" :key="file.path">
        <SourceLines :file="file" :grep="layout === 'grep'" />
      </ToolSection>
      <div v-for="note in listing!.notes" :key="note" class="note">{{ note }}</div>
      <ToolSection v-if="layout === 'grep'">
        <ToolChip>{{ t('tool.matches', { matches: countOf('matches'), files: countOf('files') }) }}</ToolChip>
        <ToolChip v-if="truncated">{{ t('tool.truncated') }}</ToolChip>
      </ToolSection>
    </template>

    <template v-else-if="layout === 'find'">
      <ToolSection>
        <FoundList :paths="found!.paths" />
        <div v-for="note in found!.notes" :key="note" class="note">{{ note }}</div>
      </ToolSection>
      <ToolSection>
        <ToolChip>{{ t('tool.results', { count: countOf('count') }) }}</ToolChip>
        <ToolChip v-if="truncated">{{ t('tool.truncated') }}</ToolChip>
      </ToolSection>
    </template>

    <template v-else-if="layout === 'webfetch'">
      <ToolSection>
        <div class="path">
          <span class="url">{{ url }}</span>
          <button type="button" class="open" :aria-label="t('tool.openUrl')" @click="open(url!)">
            <Icon icon="external-link-16" />
          </button>
          <ToolChip v-if="status !== undefined" :danger="status >= 400">{{ status }}</ToolChip>
          <ToolChip v-if="bytes !== undefined">{{ bytes }} B</ToolChip>
        </div>
      </ToolSection>
      <ToolOutput :block="block" />
    </template>

    <template v-else-if="layout === 'peer'">
      <ToolSection>
        <div class="peer">
          <Icon :icon="peerIcon!" class="info" />
          <span class="info strong">{{ peer }}</span>
        </div>
        <blockquote v-if="message" class="quote">{{ message }}</blockquote>
      </ToolSection>
      <ToolOutput :block="block" />
    </template>

    <template v-else>
      <ToolSection v-if="inputText" :label="t('tool.input')">
        <ToolText :text="inputText" tone="info" />
      </ToolSection>
      <ToolOutput :block="block" />
    </template>

    <ToolSection v-if="block.images.length > 0"><ImageThumbs :images="block.images" /></ToolSection>
  </div>
</template>

<style scoped>
.body {
  padding: 10px 14px;
  border-top: 1px solid var(--border);
}

.path {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  font: var(--small-font-size) / 18px var(--font-mono);
}

.url {
  min-width: 0;
  overflow-wrap: anywhere;
  color: var(--info);
}

.open {
  display: grid;
  place-items: center;
  color: var(--text-muted);
}

.added {
  color: var(--diff-added);
}

.removed {
  color: var(--diff-removed);
}

.note {
  margin-top: 6px;
  color: var(--text-muted);
  font: var(--small-font-size) / 18px var(--font-mono);
}

.peer {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.info {
  color: var(--info);
}

.strong {
  font-weight: bold;
}

.quote {
  margin: 8px 0 0;
  padding-left: 10px;
  border-left: 3px solid var(--info);
  font-size: 13px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
