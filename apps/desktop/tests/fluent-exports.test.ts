import { describe, expect, it } from "vitest";
import * as components from "@fluentui/react-components";
import * as icons from "@fluentui/react-icons";

const componentNames = [
  "Badge", "Button", "Card", "CardFooter", "CardHeader", "Dialog", "DialogActions",
  "DialogBody", "DialogContent", "DialogSurface", "DialogTitle", "Dropdown", "FluentProvider",
  "Input", "Label", "MessageBar", "MessageBarBody", "MessageBarTitle", "Option", "Spinner",
  "Switch", "Tab", "TabList", "Text", "Tooltip",
] as const;
const iconNames = [
  "BroadActivityFeed24Regular", "ArrowSync24Regular", "CheckmarkCircle24Regular", "Copy24Regular",
  "ErrorCircle24Regular", "FolderOpen24Regular", "Home24Regular", "Open24Regular",
  "Play24Regular", "Search24Regular", "Settings24Regular", "Stop24Regular", "Warning24Regular",
] as const;

describe("Fluent imports used by the desktop view", () => {
  it("resolve to renderable exports", () => {
    expect(componentNames.filter((name) => components[name] === undefined)).toEqual([]);
    expect(iconNames.filter((name) => icons[name] === undefined)).toEqual([]);
  });
});
