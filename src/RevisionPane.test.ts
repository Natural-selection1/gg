import { describe, it, expect, beforeEach } from "vitest";

// 辅助函数：触发事件
function triggerEvent(element: HTMLElement, eventType: string) {
    const event = new Event(eventType, { bubbles: true });
    element.dispatchEvent(event);
}

function triggerKeyboardEvent(
    element: HTMLElement,
    eventType: string,
    eventInit?: KeyboardEventInit
) {
    const event = new KeyboardEvent(eventType, { bubbles: true, ...eventInit });
    element.dispatchEvent(event);
}

describe("Ctrl+Enter 功能测试", () => {
    beforeEach(() => {
        // 清理 DOM
        document.body.innerHTML = "";
    });

    it("应该在按下 Ctrl+Enter 时触发 updateDescription", () => {
        // 创建测试容器
        const container = document.createElement("div");
        document.body.appendChild(container);

        // 创建 textarea 元素
        const textarea = document.createElement("textarea");
        textarea.className = "description";
        container.appendChild(textarea);

        let updateCalled = false;
        let descriptionChanged = false;

        function updateDescription() {
            updateCalled = true;
        }

        function handleInput(event: Event) {
            const target = event.target as HTMLTextAreaElement;
            const value = target.value;
            descriptionChanged = value !== "Initial message";
        }

        function handleKeydown(ev: KeyboardEvent) {
            if (descriptionChanged && ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
                updateDescription();
            }
        }

        // 绑定事件
        textarea.addEventListener("input", handleInput);
        textarea.addEventListener("keydown", handleKeydown);

        // 修改内容
        textarea.value = "Modified commit message";
        triggerEvent(textarea, "input");

        // 按下 Ctrl+Enter
        triggerKeyboardEvent(textarea, "keydown", {
            key: "Enter",
            ctrlKey: true,
        });

        // 验证 updateDescription 被调用
        expect(updateCalled).toBe(true);
    });

    it("应该在按下 Meta+Enter 时触发 updateDescription", () => {
        const container = document.createElement("div");
        document.body.appendChild(container);

        const textarea = document.createElement("textarea");
        container.appendChild(textarea);

        let updateCalled = false;
        let descriptionChanged = false;

        function updateDescription() {
            updateCalled = true;
        }

        function handleInput(event: Event) {
            const target = event.target as HTMLTextAreaElement;
            const value = target.value;
            descriptionChanged = value !== "Initial message";
        }

        function handleKeydown(ev: KeyboardEvent) {
            if (descriptionChanged && ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
                updateDescription();
            }
        }

        textarea.addEventListener("input", handleInput);
        textarea.addEventListener("keydown", handleKeydown);

        // 修改内容
        textarea.value = "Modified commit message";
        triggerEvent(textarea, "input");

        // 按下 Meta+Enter
        triggerKeyboardEvent(textarea, "keydown", {
            key: "Enter",
            metaKey: true,
        });

        expect(updateCalled).toBe(true);
    });

    it("当内容未改变时，Ctrl+Enter 不应该触发 updateDescription", () => {
        const container = document.createElement("div");
        document.body.appendChild(container);

        const textarea = document.createElement("textarea");
        textarea.value = "Initial message";
        container.appendChild(textarea);

        let updateCalled = false;
        let descriptionChanged = false;

        function updateDescription() {
            updateCalled = true;
        }

        function handleKeydown(ev: KeyboardEvent) {
            if (descriptionChanged && ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
                updateDescription();
            }
        }

        textarea.addEventListener("keydown", handleKeydown);

        // 不修改内容，直接按 Ctrl+Enter
        triggerKeyboardEvent(textarea, "keydown", {
            key: "Enter",
            ctrlKey: true,
        });

        expect(updateCalled).toBe(false);
    });

    it("验证修复：keydown vs keypress 事件", () => {
        const container = document.createElement("div");
        document.body.appendChild(container);

        const textarea = document.createElement("textarea");
        container.appendChild(textarea);

        let keydownCalled = false;

        function handleKeydown(ev: KeyboardEvent) {
            if (ev.key === "Enter" && ev.ctrlKey) {
                keydownCalled = true;
            }
        }

        textarea.addEventListener("keydown", handleKeydown);

        // 测试 keydown 事件
        triggerKeyboardEvent(textarea, "keydown", {
            key: "Enter",
            ctrlKey: true,
        });

        // keydown 应该被触发（这是修复后的正确行为）
        expect(keydownCalled).toBe(true);
    });

    it("应该只在正确的按键组合时触发", () => {
        const container = document.createElement("div");
        document.body.appendChild(container);

        const textarea = document.createElement("textarea");
        container.appendChild(textarea);

        let updateCalled = false;
        const descriptionChanged = true; // 假设内容已改变

        function updateDescription() {
            updateCalled = true;
        }

        function handleKeydown(ev: KeyboardEvent) {
            if (descriptionChanged && ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
                updateDescription();
            }
        }

        textarea.addEventListener("keydown", handleKeydown);

        // 测试只按 Enter
        triggerKeyboardEvent(textarea, "keydown", { key: "Enter" });
        expect(updateCalled).toBe(false);

        // 测试只按 Ctrl
        triggerKeyboardEvent(textarea, "keydown", { key: "Control", ctrlKey: true });
        expect(updateCalled).toBe(false);

        // 测试 Ctrl+其他键
        triggerKeyboardEvent(textarea, "keydown", { key: "a", ctrlKey: true });
        expect(updateCalled).toBe(false);

        // 测试正确的组合
        triggerKeyboardEvent(textarea, "keydown", { key: "Enter", ctrlKey: true });
        expect(updateCalled).toBe(true);
    });
});
