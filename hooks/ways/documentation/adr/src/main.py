# ============================================================================
# Main
# ============================================================================

def main():
    parser = argparse.ArgumentParser(
        description='ADR - Agent Decision Record CLI Tool',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__
    )
    parser.add_argument('--version', action='version',
                        version=f'adr-tool {TOOL_VERSION}')
    subparsers = parser.add_subparsers(dest='command', help='Command')

    # list
    p_list = subparsers.add_parser('list', aliases=['ls'], help='List ADRs')
    p_list.add_argument('--domain', '-d', help='Filter by domain')
    p_list.add_argument('--status', '-s', help='Filter by status')
    p_list.add_argument('--group', '-g', action='store_true',
                        help='Group by domain')
    list_scope = p_list.add_mutually_exclusive_group()
    list_scope.add_argument('--archived', action='store_true',
                            help='List archived ADRs only')
    list_scope.add_argument('--all', action='store_true',
                            help='List active and archived ADRs')
    p_list.add_argument('--field', action='append', metavar='KEY[=VALUE]',
                        help='Filter by frontmatter: KEY present, or KEY equal to or listing VALUE (repeatable)')
    p_list.add_argument('--kind', help='Filter by kind (same as --field kind=KIND)')
    p_list.add_argument('--verb', help='Filter by verb (same as --field verb=VERB)')
    p_list.add_argument('--capability', help='Filter by capability, listed or single (same as --field capability=NAME)')
    p_list.add_argument('--group-by', dest='group_by', metavar='KEY',
                        help='Group by a frontmatter field; a record listing several values is in each group')
    p_list.add_argument('--json', action='store_true',
                        help='Machine output: number, title, path, status and frontmatter for each record')

    # view
    p_view = subparsers.add_parser('view', aliases=['v', 'show'], help='View an ADR')
    p_view.add_argument('adr', help='ADR number (e.g., 38, 038, ADR-038)')

    # new
    p_new = subparsers.add_parser('new', help='Create new ADR')
    p_new.add_argument('domain', help='Domain (see `adr domains` for list)')
    p_new.add_argument('title', help='ADR title')
    p_new.add_argument('--kind', help='adr/v1: record kind (default: decision)')
    p_new.add_argument('--verb', help='adr/v1: decision verb (add, cut, change, retire, constrain)')
    p_new.add_argument('--capability', help='adr/v1: capability from the adr.yaml vocabulary')
    p_new.add_argument('--agent', help='adr/v1: the agent writing the record (e.g. Claude)')
    p_new.add_argument('--model', help='adr/v1: the model the agent runs on')

    # rename
    p_rename = subparsers.add_parser('rename', help='Rename an ADR title and/or file slug')
    p_rename.add_argument('adr', help='ADR number (e.g., 302, ADR-302)')
    p_rename.add_argument('title', nargs='?', help='New title (updates the # ADR-NNN: heading)')
    p_rename.add_argument('--slug', help='Override the filename slug (default: derived from title)')

    # lint
    p_lint = subparsers.add_parser('lint', help='Lint ADR files')
    p_lint.add_argument('paths', nargs='*', help='Specific files to lint')
    p_lint.add_argument('--check', action='store_true', help='Exit 1 if errors (CI mode)')

    # import (ADR-306)
    p_import = subparsers.add_parser('import', help='Import records through import sheets')
    import_sub = p_import.add_subparsers(dest='import_command')
    p_scan = import_sub.add_parser('scan', help='Write an import sheet for each record')
    p_scan.add_argument('paths', nargs='+', help='Record files or directories of records')
    p_scan.add_argument('--force', action='store_true',
                        help='Overwrite a sheet that differs from a fresh scan, discarding its edits')
    p_apply = import_sub.add_parser('apply', help='Write finished sheets as adr/v1 records')
    p_apply.add_argument('sheets', nargs='*', help='Sheets to apply (default: every sheet in .import/)')
    p_apply.add_argument('--partial', action='store_true',
                         help='Apply sheets with open todo items too, except items lint cannot '
                              'find again afterwards: a status with no mapping, a Deprecated '
                              'note, a number or domain mismatch')
    p_apply.add_argument('--force', action='store_true',
                         help='Overwrite a record that has uncommitted changes')
    p_apply.add_argument('--dry-run', action='store_true',
                         help='Write, lint inside the corpus and print each issue, then restore '
                              'every file and keep every sheet')

    # index
    index_parser = subparsers.add_parser('index', help='Generate ADR index')
    index_parser.add_argument('-y', '--yes', action='store_true',
                              help='Update without prompting')

    # domains
    subparsers.add_parser('domains', help='List domain number series')

    # domain (ADR-306 §6)
    p_domain = subparsers.add_parser('domain', help='Add, rename or move domains')
    domain_sub = p_domain.add_subparsers(dest='domain_command')
    p_dadd = domain_sub.add_parser('add', help='Add a domain to adr.yaml')
    p_dadd.add_argument('name', help='Domain key (e.g. ops)')
    p_dadd.add_argument('--range', required=True, help='Number range for new records, A-B (e.g. 400-499)')
    p_dadd.add_argument('--folder', required=True, help='Folder under docs/architecture')
    p_dadd.add_argument('--label', help='Display name (default: the key, capitalized)')
    p_dadd.add_argument('--description', help='One line on what the domain covers')
    p_dren = domain_sub.add_parser('rename', help='Rename a domain, and its folder, in place')
    p_dren.add_argument('old', help='Current domain key')
    p_dren.add_argument('new', help='New domain key')
    p_dren.add_argument('--folder', help='New folder (default: the new key when the folder was the old key)')
    p_dren.add_argument('--dry-run', action='store_true', help='Report what would change without changing it')
    p_dmove = domain_sub.add_parser('move', help='Move records to another domain; numbers never change')
    p_dmove.add_argument('record', nargs='?', help='ADR number (e.g. 104, ADR-104)')
    p_dmove.add_argument('domain', nargs='?', help='Target domain')
    p_dmove.add_argument('--plan', help='YAML list of {record, domain} moves, applied together')
    p_dmove.add_argument('--dry-run', action='store_true', help='Print the moves and the rewrites per file; write nothing')

    # archive
    p_archive = subparsers.add_parser(
        'archive', help='Archive an ADR out of the active set')
    p_archive.add_argument('adr', help='ADR number (e.g., 38, 038, ADR-038)')
    p_archive.add_argument('--reason', required=True,
                           help='Why this ADR leaves the active set (recorded in the banner)')
    p_archive.add_argument('--superseded-by', dest='superseded_by',
                           help='Superseding ADR(s), comma-separated (e.g., ADR-51 or 51,52#4)')
    p_archive.add_argument('--status',
                           help='Archive status (default: Superseded; validated against adr.yaml)')
    p_archive.add_argument('--dry-run', action='store_true',
                           help='Report what would change without changing it')

    # config
    subparsers.add_parser('config', help='Show configuration')

    # lifecycle (adr/v1, ADR-304 §2)
    p_accept = subparsers.add_parser('accept', help='Accept a proposed adr/v1 record')
    p_accept.add_argument('adr', help='ADR number (e.g., 101, ADR-101)')
    p_accept.add_argument('--dry-run', action='store_true', help='Check and report without writing')
    for verb, help_text in (('reject', 'Reject a proposed adr/v1 record: considered and declined'),
                            ('abandon', 'Abandon a proposed adr/v1 record: dropped before a decision')):
        p_close = subparsers.add_parser(verb, help=help_text)
        p_close.add_argument('adr', help='ADR number (e.g., 101, ADR-101)')
        p_close.add_argument('--reason', help='Why (required; appended as a Closure section)')
        p_close.add_argument('--dry-run', action='store_true', help='Report without writing')

    # record edits (adr/v1): consider, set, supersede, enact
    p_consider = subparsers.add_parser('consider', help="Append a considered entry: the operator's answer (ADR-304 §12)")
    p_consider.add_argument('adr', help='ADR number (e.g., 101, ADR-101)')
    p_consider.add_argument('--said', help='What the operator said, verbatim (required)')
    p_consider.add_argument('--via', help='Where it was said, such as a PR or a session (required)')
    p_consider.add_argument('--operator', help='Who said it (default: the gh or git user)')
    p_consider.add_argument('--covers', nargs='*', metavar='PROBE',
                            help='Probe names from the Summary the answer covers (none given writes covers: [])')
    p_consider.add_argument('--paraphrase', action='store_true', help='said is a summary, not the words')
    p_consider.add_argument('--canary', choices=['caught', 'missed'], help='Whether the operator caught the canary')
    p_consider.add_argument('--dry-run', action='store_true', help='Show the change without writing')
    p_set = subparsers.add_parser('set', help='Edit frontmatter fields: key=value, key+=item, key-=item')
    p_set.add_argument('adr', help='ADR number (e.g., 101, ADR-101)')
    p_set.add_argument('assignments', nargs='+', metavar='key=value',
                       help='Values are YAML: status=superseded, capability=[a, b], related+=ADR-7')
    p_set.add_argument('--force', action='store_true',
                       help='Edit a frozen field anyway, for migration cleanup (lint still reports it)')
    p_set.add_argument('--dry-run', action='store_true', help='Show the change without writing')
    p_supersede = subparsers.add_parser('supersede', help='Record a supersession on both records (ADR-304 §3)')
    p_supersede.add_argument('adr', help='The record replaced (e.g., 101, ADR-101)')
    p_supersede.add_argument('--by', required=True, help='The record that replaces it')
    p_supersede.add_argument('--amends', metavar='SECTION',
                             help='Replace one section only: amends: [OLD#SECTION] on the new record')
    p_supersede.add_argument('--force', action='store_true',
                             help="Write the edge on an accepted record whose kind freezes it")
    p_supersede.add_argument('--dry-run', action='store_true', help='Show the change without writing')
    p_enact = subparsers.add_parser('enact', help='Mark an accepted cut or retire done at a commit (ADR-304 §5)')
    p_enact.add_argument('adr', help='ADR number (e.g., 111, ADR-111)')
    p_enact.add_argument('commit', help='The commit hash that finished the removal')
    p_enact.add_argument('--dry-run', action='store_true', help='Show the change without writing')

    # cite
    p_cite = subparsers.add_parser('cite', help='Check ADR citations in code against the records')
    p_cite.add_argument('paths', nargs='*', help='Limit the scan to these files or directories')
    p_cite.add_argument('--check', action='store_true', help='Exit 1 if errors (CI mode)')
    p_cite.add_argument('--no-inventory', action='store_true',
                        help="Skip surface inventories (they run shell commands from adr.yaml)")

    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        return 0

    commands = {
        'list': cmd_list,
        'ls': cmd_list,
        'view': cmd_view,
        'v': cmd_view,
        'show': cmd_view,
        'new': cmd_new,
        'rename': cmd_rename,
        'lint': cmd_lint,
        'index': cmd_index,
        'archive': cmd_archive,
        'domains': cmd_domains,
        'domain': cmd_domain,
        'config': cmd_config,
        'cite': cmd_cite,
        'accept': cmd_accept,
        'reject': cmd_reject,
        'abandon': cmd_abandon,
        'import': cmd_import,
        'consider': cmd_consider,
        'set': cmd_set,
        'supersede': cmd_supersede,
        'enact': cmd_enact,
    }

    return commands[args.command](args)

if __name__ == '__main__':
    sys.exit(main())
