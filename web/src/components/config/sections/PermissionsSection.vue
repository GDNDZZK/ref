<script setup lang="ts">
// Permissions & Delays configuration section (`permission` and `delay`).
import { useI18n } from 'vue-i18n'
import { getAt, setAt, type JsonObject } from '../../../config/jsonPatch.ts'
import { FormRow, PermissionRulesEditor, ToolDelaysEditor } from '../index.ts'

const props = defineProps<{ doc: JsonObject }>()
const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

function patch(key: string, value: unknown): void {
  emit('update:doc', setAt(props.doc, [key], value))
}
</script>

<template>
  <div class="section-body">
    <FormRow
      vertical
      :label="t('config.editor.permissions.globalPermission')"
      :description="t('config.editor.permissions.globalPermissionDesc')"
    >
      <PermissionRulesEditor
        :model-value="getAt(doc, ['permission'])"
        :label="t('config.editor.permissions.globalPermission')"
        @update:model-value="patch('permission', $event)"
      />
    </FormRow>

    <FormRow
      vertical
      :label="t('config.editor.permissions.globalDelay')"
      :description="t('config.editor.permissions.globalDelayDesc')"
      class="divider"
    >
      <ToolDelaysEditor :model-value="getAt(doc, ['delay'])" @update:model-value="patch('delay', $event)" />
    </FormRow>
  </div>
</template>

<style scoped>
.section-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.divider {
  margin-top: 8px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
}
</style>
