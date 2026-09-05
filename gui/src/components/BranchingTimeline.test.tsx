import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { BranchingTimeline } from './BranchingTimeline';
import { Version } from '@/models/version';

function makeVersion(id: number, versionNumber: number, parentId?: number, tag?: string): Version {
  const v = new Version(versionNumber, 1);
  v.id = id;
  v.parent_id = parentId;
  v.created_at = new Date(2024, 0, versionNumber);
  v.tag = tag;
  return v;
}

describe('BranchingTimeline', () => {
  it('renders no version rows for an empty version list', () => {
    render(<BranchingTimeline versions={[]} activeVersionId={undefined} />);
    expect(screen.queryByText(/^v\d{3}$/)).toBeNull();
  });

  it('renders all nodes for a linear chain', () => {
    const versions = [
      makeVersion(1, 1),
      makeVersion(2, 2, 1),
      makeVersion(3, 3, 2),
    ];
    render(<BranchingTimeline versions={versions} activeVersionId={3} />);
    expect(screen.getByText('v001')).toBeInTheDocument();
    expect(screen.getByText('v002')).toBeInTheDocument();
    expect(screen.getByText('v003')).toBeInTheDocument();
  });

  it('shows CURRENT badge on the active version', () => {
    const versions = [makeVersion(1, 1), makeVersion(2, 2, 1)];
    render(<BranchingTimeline versions={versions} activeVersionId={2} />);
    expect(screen.getByText('CURRENT')).toBeInTheDocument();
  });

  it('pre-branching null-parent versions are not marked inactive', () => {
    const versions = [
      makeVersion(1, 1),
      makeVersion(2, 2),
      makeVersion(3, 3),
    ];
    const { container } = render(
      <BranchingTimeline versions={versions} activeVersionId={3} />
    );
    const rows = container.querySelectorAll('[style*="opacity"]');
    // No node should have opacity 0.45
    rows.forEach(row => {
      expect((row as HTMLElement).style.opacity).not.toBe('0.45');
    });
  });

  it('branched-off versions are marked inactive (reduced opacity)', () => {
    const versions = [
      makeVersion(1, 1),
      makeVersion(2, 2, 1),
      makeVersion(3, 3, 2),
      makeVersion(4, 4, 2),
    ];
    render(<BranchingTimeline versions={versions} activeVersionId={3} />);
    const nodes = document.querySelectorAll<HTMLElement>('[style*="opacity: 0.45"]');
    // Version 4 is an inactive branch (diverges from v2 but active is v3)
    expect(nodes.length).toBeGreaterThanOrEqual(1);
  });

  it('no versions are inactive when there is no active version', () => {
    const versions = [makeVersion(1, 1), makeVersion(2, 2, 1)];
    const { container } = render(
      <BranchingTimeline versions={versions} activeVersionId={undefined} />
    );
    const inactiveNodes = container.querySelectorAll('[style*="opacity: 0.45"]');
    expect(inactiveNodes.length).toBe(0);
  });

  it('clicking tag text starts editing', () => {
    const versions = [makeVersion(1, 1, undefined, 'My Tag')];
    render(<BranchingTimeline versions={versions} activeVersionId={1} />);
    const tagEl = screen.getByText('My Tag');
    fireEvent.click(tagEl);
    expect(screen.getByDisplayValue('My Tag')).toBeInTheDocument();
  });

  it('pressing Escape cancels tag editing', () => {
    const versions = [makeVersion(1, 1, undefined, 'My Tag')];
    render(<BranchingTimeline versions={versions} activeVersionId={1} />);
    fireEvent.click(screen.getByText('My Tag'));
    const input = screen.getByDisplayValue('My Tag');
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByDisplayValue('My Tag')).toBeNull();
    expect(screen.getByText('My Tag')).toBeInTheDocument();
  });

  it('calls onTag when Enter is pressed in edit field', () => {
    const onTag = vi.fn();
    const versions = [makeVersion(1, 1, undefined, 'Old Tag')];
    render(<BranchingTimeline versions={versions} activeVersionId={1} onTag={onTag} />);
    fireEvent.click(screen.getByText('Old Tag'));
    const input = screen.getByDisplayValue('Old Tag');
    fireEvent.change(input, { target: { value: 'New Tag' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onTag).toHaveBeenCalledWith(versions[0], 'New Tag');
  });

  it('calls onRestore when Restore button is clicked', () => {
    const onRestore = vi.fn();
    const versions = [makeVersion(1, 1), makeVersion(2, 2, 1)];
    render(<BranchingTimeline versions={versions} activeVersionId={2} onRestore={onRestore} />);
    // Hover to reveal the Restore button on version 1
    const row = screen.getByText('v001').closest('.group');
    if (row) fireEvent.mouseEnter(row);
    const restoreButtons = screen.getAllByRole('button', { name: /restore/i });
    fireEvent.click(restoreButtons[0]);
    expect(onRestore).toHaveBeenCalled();
  });

  it('can render as a read-only preview without restore actions', () => {
    const versions = [makeVersion(1, 1), makeVersion(2, 2, 1)];
    render(<BranchingTimeline versions={versions} activeVersionId={2} showActions={false} />);

    expect(screen.queryByRole('button', { name: /restore/i })).toBeNull();
  });

  it('active version appears at its correct temporal position, not forced to top', () => {
    // v3 is active (restored) but has an older created_at than v4 and v5.
    // It should appear at position 2 (after v5 and v4) so tree edges always point downward.
    const versions = [
      makeVersion(1, 1),
      makeVersion(2, 2, 1),
      makeVersion(3, 3, 2), // active, date = Jan 3
      makeVersion(4, 4, 2), // inactive branch, date = Jan 4
      makeVersion(5, 5, 2), // inactive branch, date = Jan 5 (newest)
    ];
    render(<BranchingTimeline versions={versions} activeVersionId={3} />);
    const labels = screen.getAllByText(/^v\d{3}$/);
    // Sorted by date descending: v5, v4, v3 (active), v2, v1
    expect(labels[0].textContent).toBe('v005');
    expect(labels[1].textContent).toBe('v004');
    expect(labels[2].textContent).toBe('v003');
  });
});
