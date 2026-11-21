import { HttpException, HttpStatus, Logger } from '@nestjs/common';
import { AllExceptionsFilter } from './all-exceptions.filter';

function makeHost(
  statusFn: jest.Mock,
  jsonFn: jest.Mock,
  method = 'GET',
  url = '/test',
) {
  return {
    switchToHttp: () => ({
      getResponse: () => ({
        status: statusFn,
        // status() must return the same object so that .json() can be chained.
        // We reset statusFn to return an object with jsonFn attached.
      }),
      getRequest: () => ({ method, url }),
    }),
  } as any;
}

/**
 * Returns a mock ArgumentsHost whose response.status(n) returns an object
 * with a json() method — matching the Express fluent API used by the filter.
 */
function buildHost(
  statusMock: jest.Mock,
  jsonMock: jest.Mock,
  method = 'GET',
  url = '/test',
) {
  const responseMock = {
    status: (code: number) => {
      statusMock(code);
      return { json: jsonMock };
    },
  };

  return {
    switchToHttp: () => ({
      getResponse: () => responseMock,
      getRequest: () => ({ method, url }),
    }),
  } as any;
}

describe('AllExceptionsFilter', () => {
  let filter: AllExceptionsFilter;

  beforeEach(() => {
    filter = new AllExceptionsFilter();
    // Silence internal logger output during tests.
    jest.spyOn(Logger.prototype, 'error').mockImplementation(() => undefined);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  describe('unhandled (non-HTTP) exceptions', () => {
    it('returns 500 with a generic message and does not leak the error detail', () => {
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);

      filter.catch(new Error('PostgreSQL: relation "users" does not exist'), host);

      expect(statusMock).toHaveBeenCalledWith(HttpStatus.INTERNAL_SERVER_ERROR);
      const body = jsonMock.mock.calls[0][0];
      expect(body.statusCode).toBe(500);
      expect(body.message).toBe('Internal server error');
      expect(body.timestamp).toBeDefined();
      // Ensure no database detail leaks into the response.
      expect(JSON.stringify(body)).not.toContain('PostgreSQL');
      expect(JSON.stringify(body)).not.toContain('relation');
    });

    it('logs the stack trace internally', () => {
      const logSpy = jest.spyOn(Logger.prototype, 'error');
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);
      const err = new Error('internal db failure');

      filter.catch(err, host);

      expect(logSpy).toHaveBeenCalledWith(
        expect.stringContaining('GET /test'),
        err.stack,
      );
    });

    it('handles non-Error thrown values without crashing', () => {
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);

      // Throwing a plain string is unusual but must not crash the filter.
      filter.catch('something went wrong', host);

      expect(statusMock).toHaveBeenCalledWith(HttpStatus.INTERNAL_SERVER_ERROR);
    });
  });

  describe('HttpExceptions', () => {
    it('preserves the HTTP status and message of an HttpException', () => {
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);

      filter.catch(new HttpException('Not Found', HttpStatus.NOT_FOUND), host);

      expect(statusMock).toHaveBeenCalledWith(HttpStatus.NOT_FOUND);
      const body = jsonMock.mock.calls[0][0];
      expect(body.statusCode).toBe(404);
      expect(body.message).toBe('Not Found');
    });

    it('flattens a ValidationPipe-style response object into a top-level message', () => {
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);
      const payload = {
        statusCode: 400,
        message: ['email must be an email'],
        error: 'Bad Request',
      };

      filter.catch(new HttpException(payload, HttpStatus.BAD_REQUEST), host);

      expect(statusMock).toHaveBeenCalledWith(HttpStatus.BAD_REQUEST);
      const body = jsonMock.mock.calls[0][0];
      expect(body.statusCode).toBe(400);
      expect(body.message).toEqual(['email must be an email']);
      expect(body.timestamp).toBeDefined();
      // No redundant nesting: message must not be the whole response object.
      expect(body.message).not.toEqual(payload);
      expect(body.message).not.toHaveProperty('message');
    });

    it('extracts a string message from an object response', () => {
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);
      const payload = { statusCode: 401, message: 'Unauthorized', error: 'Unauthorized' };

      filter.catch(new HttpException(payload, HttpStatus.UNAUTHORIZED), host);

      const body = jsonMock.mock.calls[0][0];
      expect(body.statusCode).toBe(401);
      expect(body.message).toBe('Unauthorized');
    });

    it('does not log stack traces for HttpExceptions', () => {
      const logSpy = jest.spyOn(Logger.prototype, 'error');
      const statusMock = jest.fn();
      const jsonMock = jest.fn();
      const host = buildHost(statusMock, jsonMock);

      filter.catch(new HttpException('Forbidden', HttpStatus.FORBIDDEN), host);

      expect(logSpy).not.toHaveBeenCalled();
    });
  });
});
