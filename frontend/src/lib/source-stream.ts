/** Native single-file capabilities must never be presented as browser links. */
export function isSourceStreamTicket(ticket: string | null): boolean {
	return ticket?.startsWith('dashbeam-stream-v1:') ?? false
}
