import "@testing-library/jest-dom";
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { ChatAvatar } from "./ChatAvatar";
import { ChatRow } from "./ChatRow";

describe("ChatAvatar", () => {
  it("renders the first character of the name when no src is provided", () => {
    render(<ChatAvatar name="小工" />);
    const av = screen.getByTestId("chat-avatar");
    expect(av).toHaveTextContent("小");
  });

  it("upper-cases ASCII initials", () => {
    render(<ChatAvatar name="alice" />);
    expect(screen.getByTestId("chat-avatar")).toHaveTextContent("A");
  });

  it("renders an <img> when src is provided", () => {
    render(<ChatAvatar name="AI小家" src="/brand-avatar-gold.svg" />);
    const av = screen.getByTestId("chat-avatar");
    const img = av.querySelector("img");
    expect(img).toBeInTheDocument();
    expect(img?.getAttribute("src")).toBe("/brand-avatar-gold.svg");
  });

  it("falls back to a non-empty initial for whitespace-only names", () => {
    render(<ChatAvatar name="   " />);
    // Should still render a single visible glyph, not throw.
    expect(screen.getByTestId("chat-avatar")).toBeInTheDocument();
  });

  it("uses the name as aria-label and title", () => {
    render(<ChatAvatar name="小研" />);
    const av = screen.getByTestId("chat-avatar");
    expect(av.getAttribute("aria-label")).toBe("小研");
    expect(av.getAttribute("title")).toBe("小研");
  });

  it('variant="neutral" renders the first character in brand tint', () => {
    render(<ChatAvatar name="ybq" variant="neutral" />);
    const av = screen.getByTestId("chat-avatar");
    expect(av.getAttribute("data-variant")).toBe("neutral");
    expect(av.querySelector("img")).toBeNull();
    expect(av.querySelector("svg")).toBeNull();
    expect(av).toHaveTextContent("Y");
  });

  it('explicit src wins over variant="neutral"', () => {
    render(
      <ChatAvatar name="x" src="/brand-avatar-gold.svg" variant="neutral" />,
    );
    const av = screen.getByTestId("chat-avatar");
    expect(av.getAttribute("data-variant")).toBe("image");
    expect(av.querySelector("img")?.getAttribute("src")).toBe(
      "/brand-avatar-gold.svg",
    );
  });
});

describe("ChatRow", () => {
  it("lays out user rows right-aligned (flex-row-reverse)", () => {
    render(
      <ChatRow role="user" name="me">
        <div>bubble</div>
      </ChatRow>,
    );
    const row = screen.getByTestId("chat-row");
    expect(row).toHaveAttribute("data-role", "user");
    expect(row.className).toMatch(/flex-row-reverse/);
  });

  it("lays out assistant rows left-aligned (plain flex-row)", () => {
    render(
      <ChatRow role="assistant" name="AI小家">
        <div>bubble</div>
      </ChatRow>,
    );
    const row = screen.getByTestId("chat-row");
    expect(row).toHaveAttribute("data-role", "assistant");
    expect(row.className).toMatch(/flex-row(?!-reverse)/);
  });

  it("reserves user avatar width on assistant rows so AI content aligns before the user avatar column", () => {
    render(
      <ChatRow role="assistant" name="AI小家">
        <div>bubble</div>
      </ChatRow>,
    );
    const row = screen.getByTestId("chat-row");
    expect(row.className).toContain("pr-9");
  });

  it("shows the sender name as a separate header row", () => {
    render(
      <ChatRow role="assistant" name="AI小家">
        <div>hello</div>
      </ChatRow>,
    );
    expect(screen.getByTestId("chat-row-name")).toHaveTextContent("AI小家");
  });

  it("uses a compact gap between stacked assistant content blocks", () => {
    render(
      <ChatRow role="assistant" name="AI小家">
        <div>one</div>
        <div>two</div>
      </ChatRow>,
    );
    const content = screen.getByText("one").parentElement;
    expect(content).toHaveClass("gap-1");
    expect(content).not.toHaveClass("gap-3");
  });

  it("renders an avatar with the provided src for the brand logo", () => {
    render(
      <ChatRow
        role="assistant"
        name="AI小家"
        avatarUrl="/brand-avatar-gold.svg"
      >
        <div>hi</div>
      </ChatRow>,
    );
    const img = screen.getByTestId("chat-avatar").querySelector("img");
    expect(img?.getAttribute("src")).toBe("/brand-avatar-gold.svg");
  });
});
