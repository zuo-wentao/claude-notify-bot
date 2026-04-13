<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

interface Props {
  modelValue: boolean;
  disabled?: boolean;
  class?: string;
}

const props = withDefaults(defineProps<Props>(), {
  disabled: false,
  class: "",
});

const emit = defineEmits<{
  (e: "update:modelValue", value: boolean): void;
}>();

function toggle() {
  if (props.disabled) return;
  emit("update:modelValue", !props.modelValue);
}

const trackClass = computed(() =>
  cn(
    "inline-flex h-6 w-11 items-center rounded-full border-2 border-transparent transition-colors",
    props.modelValue ? "bg-primary" : "bg-slate-300",
    props.disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer",
    props.class,
  ),
);

const thumbClass = computed(() =>
  cn(
    "pointer-events-none block h-5 w-5 rounded-full bg-white shadow transition-transform",
    props.modelValue ? "translate-x-5" : "translate-x-0",
  ),
);
</script>

<template>
  <button type="button" role="switch" :aria-checked="modelValue" :disabled="disabled" :class="trackClass" @click="toggle">
    <span :class="thumbClass" />
  </button>
</template>
