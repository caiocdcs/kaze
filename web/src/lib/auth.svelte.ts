/** Set when the server rejects our API key; the layout then asks for one. */
class Auth {
	needed = $state(false);
}

export const auth = new Auth();
