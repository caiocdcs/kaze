/** Set when the server turns a request away; the layout then asks to sign in. */
class Auth {
	needed = $state(false);
}

export const auth = new Auth();
